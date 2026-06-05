use super::vfs::{FileSystem, FileHandle, OpenFlags, Result, VfsError};
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

enum MemNode {
    File(Vec<u8>),
    Dir,
}

pub struct MemFs {
    nodes: UnsafeCell<BTreeMap<String, MemNode>>,
    next_handle: AtomicUsize,
    // handle -> (normalized path, read offset)
    open_handles: UnsafeCell<BTreeMap<FileHandle, (String, usize)>>,
}

unsafe impl Send for MemFs {}
unsafe impl Sync for MemFs {}

impl MemFs {
    pub fn new() -> Self {
        let mut nodes: BTreeMap<String, MemNode> = BTreeMap::new();
        nodes.insert(String::new(), MemNode::Dir); // root marker
        Self {
            nodes: UnsafeCell::new(nodes),
            next_handle: AtomicUsize::new(1),
            open_handles: UnsafeCell::new(BTreeMap::new()),
        }
    }

    fn nodes_mut(&self) -> &mut BTreeMap<String, MemNode> {
        unsafe { &mut *self.nodes.get() }
    }

    fn nodes_ref(&self) -> &BTreeMap<String, MemNode> {
        unsafe { &*self.nodes.get() }
    }

    fn handles_mut(&self) -> &mut BTreeMap<FileHandle, (String, usize)> {
        unsafe { &mut *self.open_handles.get() }
    }

    pub fn create_dir(&self, path: &str) -> Result<()> {
        let p = normalize(path);
        let nodes = self.nodes_mut();
        if nodes.contains_key(&p) {
            return Err(VfsError::AlreadyExists);
        }
        nodes.insert(p, MemNode::Dir);
        Ok(())
    }

    pub fn create_file(&self, path: &str, data: Vec<u8>) -> Result<()> {
        let p = normalize(path);
        self.nodes_mut().insert(p, MemNode::File(data));
        Ok(())
    }
}

fn normalize(path: &str) -> String {
    let p = path.trim_start_matches('/');
    String::from(p.trim_end_matches('/'))
}

fn parent_exists(nodes: &BTreeMap<String, MemNode>, path: &str) -> bool {
    let p = normalize(path);
    if let Some(slash) = p.rfind('/') {
        let parent = &p[..slash];
        matches!(nodes.get(parent), Some(MemNode::Dir))
    } else {
        true // top-level entry, parent is root which always exists
    }
}

impl FileSystem for MemFs {
    fn open(&self, path: &str, flags: OpenFlags) -> Result<FileHandle> {
        let p = normalize(path);
        let nodes = self.nodes_mut();

        match flags {
            OpenFlags::ReadOnly => {
                match nodes.get(&p) {
                    None => return Err(VfsError::NotFound),
                    Some(MemNode::Dir) => return Err(VfsError::IsADirectory),
                    Some(MemNode::File(_)) => {}
                }
            }
            OpenFlags::WriteOnly | OpenFlags::ReadWrite => {
                match nodes.get(&p) {
                    Some(MemNode::Dir) => return Err(VfsError::IsADirectory),
                    Some(MemNode::File(_)) => {}
                    None => {
                        if !parent_exists(nodes, &p) {
                            return Err(VfsError::NotFound);
                        }
                        nodes.insert(p.clone(), MemNode::File(Vec::new()));
                    }
                }
            }
        }

        let handle = self.next_handle.fetch_add(1, Ordering::SeqCst);
        self.handles_mut().insert(handle, (p, 0));
        Ok(handle)
    }

    fn read(&self, handle: FileHandle, buf: &mut [u8]) -> Result<usize> {
        // Clone path + copy offset so we don't hold a &mut into handles while
        // borrowing nodes.
        let (path, start) = {
            let h = self.handles_mut();
            let e = h.get(&handle).ok_or(VfsError::InvalidDescriptor)?;
            (e.0.clone(), e.1)
        };

        let n = {
            let nodes = self.nodes_ref();
            match nodes.get(&path) {
                Some(MemNode::File(data)) => {
                    if start >= data.len() {
                        0
                    } else {
                        let avail = &data[start..];
                        let n = buf.len().min(avail.len());
                        buf[..n].copy_from_slice(&avail[..n]);
                        n
                    }
                }
                Some(MemNode::Dir) => return Err(VfsError::IsADirectory),
                None => return Err(VfsError::NotFound),
            }
        };

        if let Some(e) = self.handles_mut().get_mut(&handle) {
            e.1 += n;
        }
        Ok(n)
    }

    fn write(&self, handle: FileHandle, buf: &[u8]) -> Result<usize> {
        let (path, start) = {
            let h = self.handles_mut();
            let e = h.get(&handle).ok_or(VfsError::InvalidDescriptor)?;
            (e.0.clone(), e.1)
        };

        {
            let nodes = self.nodes_mut();
            match nodes.get_mut(&path) {
                Some(MemNode::File(data)) => {
                    let end = start + buf.len();
                    if end > data.len() {
                        data.resize(end, 0);
                    }
                    data[start..end].copy_from_slice(buf);
                }
                Some(MemNode::Dir) => return Err(VfsError::IsADirectory),
                None => return Err(VfsError::NotFound),
            }
        }

        if let Some(e) = self.handles_mut().get_mut(&handle) {
            e.1 += buf.len();
        }
        Ok(buf.len())
    }

    fn close(&self, handle: FileHandle) -> Result<()> {
        self.handles_mut()
            .remove(&handle)
            .ok_or(VfsError::InvalidDescriptor)?;
        Ok(())
    }

    fn mkdir(&self, path: &str) -> Result<()> {
        self.create_dir(path)
    }

    fn list_dir(&self, path: &str) -> Result<Vec<String>> {
        let p = normalize(path);
        let nodes = self.nodes_ref();

        // Verify the path is a directory
        if p.is_empty() {
            // root always valid
        } else {
            match nodes.get(&p) {
                Some(MemNode::Dir) => {}
                Some(MemNode::File(_)) => return Err(VfsError::NotADirectory),
                None => return Err(VfsError::NotFound),
            }
        }

        let prefix = if p.is_empty() {
            String::new()
        } else {
            let mut s = p.clone();
            s.push('/');
            s
        };

        let mut entries: Vec<String> = Vec::new();
        for key in nodes.keys() {
            if key.is_empty() {
                continue; // skip root marker
            }
            let rest: &str = if prefix.is_empty() {
                key.as_str()
            } else if key.starts_with(prefix.as_str()) {
                &key[prefix.len()..]
            } else {
                continue;
            };

            if !rest.is_empty() && !rest.contains('/') {
                entries.push(rest.to_string());
            }
        }
        Ok(entries)
    }
}

impl Default for MemFs {
    fn default() -> Self {
        Self::new()
    }
}
