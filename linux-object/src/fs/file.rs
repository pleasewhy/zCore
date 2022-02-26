//! File handle for process

use alloc::{boxed::Box, string::String, sync::Arc};

use async_trait::async_trait;
use spin::RwLock;

use rcore_fs::vfs::{FileType, /*FsError,*/ INode, Metadata, PollStatus};
use zircon_object::object::*;
use zircon_object::vm::{pages, VmObject};

use super::FileLike;
use crate::error::{LxError, LxResult};

bitflags::bitflags! {
    /// File open flags
    pub struct OpenFlags: usize {
        /// read only
        const RDONLY = 0;
        /// write only
        const WRONLY = 1;
        /// read write
        const RDWR = 2;
        /// create file if it does not exist
        const CREATE = 1 << 6;
        /// error if CREATE and the file exists
        const EXCLUSIVE = 1 << 7;
        /// truncate file upon open
        const TRUNCATE = 1 << 9;
        /// append on each write
        const APPEND = 1 << 10;
        /// non block open
        const NON_BLOCK = 1 << 11;
        /// close on exec
        const CLOEXEC = 1 << 19;
    }
}

impl OpenFlags {
    /// check if the OpenFlags is readable
    pub fn readable(self) -> bool {
        let b = self.bits() & 0b11;
        b == Self::RDONLY.bits() || b == Self::RDWR.bits()
    }
    /// check if the OpenFlags is writable
    pub fn writable(self) -> bool {
        let b = self.bits() & 0b11;
        b == Self::WRONLY.bits() || b == Self::RDWR.bits()
    }
    /// check if the OpenFlags caontains append
    pub fn is_append(self) -> bool {
        self.contains(Self::APPEND)
    }
    /// check if the OpenFlags caontains non-block
    pub fn non_block(self) -> bool {
        self.contains(Self::NON_BLOCK)
    }
    /// close on exec
    pub fn close_on_exec(self) -> bool {
        self.contains(Self::CLOEXEC)
    }
}

/// file seek type
#[derive(Debug)]
pub enum SeekFrom {
    /// seek from start point
    Start(u64),
    /// seek from end
    End(i64),
    /// seek from current
    Current(i64),
}

/// file inner mut data struct
#[derive(Clone)]
struct FileInner {
    /// content offset on read/write
    offset: u64,
    /// file open options
    flags: OpenFlags,
    /// file INode
    inode: Arc<dyn INode>,
}

/// file implement struct
pub struct File {
    /// object base
    base: KObjectBase,
    /// file path
    path: String,
    /// file inner mut data
    inner: RwLock<FileInner>,
}

impl_kobject!(File);

impl FileInner {
    /// read from file
    async fn read(&mut self, buf: &mut [u8]) -> LxResult<usize> {
        let len = self.read_at(self.offset, buf).await?;
        self.offset += len as u64;
        Ok(len)
    }

    /// read from file at given offset
    async fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> LxResult<usize> {
        if !self.flags.readable() {
            return Err(LxError::EBADF);
        }
        if self.flags.non_block() {
            unimplemented!();
        }
        let len = self.inode.read_at(offset as usize, buf).await?;
        Ok(len)
    }

    /// write to file
    async fn write(&mut self, buf: &[u8]) -> LxResult<usize> {
        let offset = if self.flags.is_append() {
            self.inode.metadata()?.size as u64
        } else {
            self.offset
        };
        let len = self.write_at(offset, buf).await?;
        self.offset = offset + len as u64;
        Ok(len)
    }

    /// write to file at given offset
    async fn write_at(&mut self, offset: u64, buf: &[u8]) -> LxResult<usize> {
        if !self.flags.writable() {
            return Err(LxError::EBADF);
        }
        let len = self.inode.write_at(offset as usize, buf).await?;
        Ok(len)
    }
}

impl File {
    /// create a file struct
    pub fn new(inode: Arc<dyn INode>, flags: OpenFlags, path: String) -> Arc<Self> {
        Arc::new(File {
            base: KObjectBase::new(),
            path,
            inner: RwLock::new(FileInner {
                offset: 0,
                flags,
                inode,
            }),
        })
    }

    /// Returns the file path.
    pub fn path(&self) -> &String {
        &self.path
    }

    /// seek from given type and offset
    pub fn seek(&self, pos: SeekFrom) -> LxResult<u64> {
        let mut inner = self.inner.write();
        inner.offset = match pos {
            SeekFrom::Start(offset) => offset,
            SeekFrom::End(offset) => (inner.inode.metadata()?.size as i64 + offset) as u64,
            SeekFrom::Current(offset) => (inner.offset as i64 + offset) as u64,
        };
        Ok(inner.offset)
    }

    /// resize the file
    pub async fn set_len(&self, len: u64) -> LxResult {
        let inner = self.inner.write();
        if !inner.flags.writable() {
            return Err(LxError::EBADF);
        }
        inner.inode.resize(len as usize).await?;
        Ok(())
    }

    /// Sync all data and metadata
    pub async fn sync_all(&self) -> LxResult {
        self.inner.read().inode.sync_all().await?;
        Ok(())
    }

    /// Sync data (not include metadata)
    pub async fn sync_data(&self) -> LxResult {
        self.inner.read().inode.sync_data().await?;
        Ok(())
    }

    /// get metadata of file
    /// fstat
    pub fn metadata(&self) -> LxResult<Metadata> {
        Ok(self.inner.read().inode.metadata()?)
    }

    /// lookup the file following the link
    pub async fn lookup_follow(&self, path: &str, max_follow: usize) -> LxResult<Arc<dyn INode>> {
        Ok(self
            .inner
            .read()
            .inode
            .lookup_follow(path, max_follow)
            .await?)
    }

    /// get the name of dir entry
    pub async fn read_entry(&self) -> LxResult<String> {
        let mut inner = self.inner.write();
        if !inner.flags.readable() {
            return Err(LxError::EBADF);
        }
        let name = inner.inode.get_entry(inner.offset as usize).await?;
        inner.offset += 1;
        Ok(name)
    }

    /// get INode of this file
    pub fn inode(&self) -> Arc<dyn INode> {
        self.inner.read().inode.clone()
    }
}

#[async_trait]
impl FileLike for File {
    async fn flags(&self) -> OpenFlags {
        self.inner.read().flags
    }

    async fn set_flags(&self, f: OpenFlags) -> LxResult {
        let flags = &mut self.inner.write().flags;
        flags.set(OpenFlags::APPEND, f.contains(OpenFlags::APPEND));
        flags.set(OpenFlags::NON_BLOCK, f.contains(OpenFlags::NON_BLOCK));
        flags.set(OpenFlags::CLOEXEC, f.contains(OpenFlags::CLOEXEC));
        Ok(())
    }

    async fn dup(&self) -> Arc<dyn FileLike> {
        Arc::new(Self {
            base: KObjectBase::new(),
            path: self.path.clone(),
            inner: RwLock::new(self.inner.read().clone()),
        })
    }

    async fn read(&self, buf: &mut [u8]) -> LxResult<usize> {
        self.inner.write().read(buf).await
    }

    async fn write(&self, buf: &[u8]) -> LxResult<usize> {
        self.inner.write().write(buf).await
    }

    async fn read_at(&self, offset: u64, buf: &mut [u8]) -> LxResult<usize> {
        self.inner.write().read_at(offset, buf).await
    }

    async fn write_at(&self, offset: u64, buf: &[u8]) -> LxResult<usize> {
        self.inner.write().write_at(offset, buf).await
    }

    async fn poll(&self) -> LxResult<PollStatus> {
        Ok(self.inner.read().inode.poll()?)
    }

    async fn async_poll(&self) -> LxResult<PollStatus> {
        Ok(self.inner.read().inode.async_poll().await?)
    }

    async fn ioctl(&self, request: usize, arg1: usize, _arg2: usize, _arg3: usize) -> LxResult<usize> {
        // ioctl syscall
        self.inner.read().inode.io_control(request as u32, arg1)?;
        Ok(0)
    }

    /// Returns the [`VmObject`] representing the file with given `offset` and `len`.
    async fn get_vmo(&self, offset: usize, len: usize) -> LxResult<Arc<VmObject>> {
        let inner = self.inner.read();
        match inner.inode.metadata()?.type_ {
            FileType::File => {
                // TODO: better implementation
                let mut buf = alloc::vec![0; len];
                let len = inner.inode.read_at(offset, &mut buf).await?;
                let vmo = VmObject::new_paged(pages(len));
                vmo.write(0, &buf[..len])?;
                Ok(vmo)
            }
            FileType::CharDevice => {
                use super::devfs::FbDev;
                if let Some(fbdev) = inner.inode.downcast_ref::<FbDev>() {
                    fbdev.get_vmo(offset, len)
                } else {
                    Err(LxError::ENOSYS)
                }
            }
            _ => Err(LxError::ENOSYS),
        }
    }
}
