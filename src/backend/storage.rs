use log::{debug, error, warn};
use ruffle_core::backend::storage::StorageBackend;
use rust_libretro::environment;
use rust_libretro::types::{VfsFileOpenFlags, VfsFileOpenHints};
use rust_libretro_sys::{retro_environment_t, retro_vfs_interface, retro_vfs_interface_info};
use std::error::Error;
use std::ffi::{c_int, CString};
use std::path::{Component, Path, PathBuf};
use std::ptr;
use thiserror::Error as ThisError;

#[derive(ThisError, Debug)]
pub enum StorageError {
    #[error("Failed to get VFS interface v{0}")]
    FailedToGetInterface(u32),

    #[error("Save data path is invalid Unicode")]
    InvalidUnicodePath,

    #[error("Could not get pointer for VFS operation {0}")]
    OperationUnavailable(&'static str),

    #[error("Error {0} in calling vfs_mkdir({1})")]
    MkdirError(c_int, PathBuf),
}

pub struct RetroVfsStorageBackend {
    base_path: PathBuf,
    shared_objects_path: PathBuf,
    vfs: retro_vfs_interface_info,
}

impl RetroVfsStorageBackend {
    pub fn new(base_path: &Path, environment: retro_environment_t) -> Result<Self, Box<dyn Error>> {
        let shared_objects_path = base_path.join("SharedObjects");
        let vfs = unsafe {
            environment::get_vfs_interface(
                environment,
                retro_vfs_interface_info {
                    required_interface_version: 3,
                    iface: ptr::null_mut(),
                },
            )
        }
        .and_then(|vfs| if vfs.iface.is_null() { None } else { Some(vfs) })
        .ok_or(StorageError::FailedToGetInterface(3))?;
        // Get the VFS interface and ensure that the returned pointer is non-null

        return unsafe {
            match Self::ensure_storage_dir(*vfs.iface, &shared_objects_path) {
                Ok(_) => Ok(Self {
                    base_path: PathBuf::from(base_path),
                    shared_objects_path,
                    vfs,
                }),
                Err(error) => Err(error),
            }
        };
    }

    /// Verifies that the path contains no `..` components to prevent accessing files outside of the Ruffle directory.
    fn is_path_allowed(path: &Path) -> bool {
        path.components().all(|c| c != Component::ParentDir)
    }

    fn get_shared_object_path(&self, name: &str) -> PathBuf {
        self.shared_objects_path.join(format!("{name}.sol"))
    }

    fn get_back_compat_shared_object_path(&self, name: &str) -> PathBuf {
        // Backwards compatibility with pre-05/09/2021:
        // Search for data in old location, without .sol extension and # prefix.
        // Remove this code eventually.
        self.base_path.join(name.replacen("/#", "/", 1))
    }

    fn ensure_storage_dir(vfs: retro_vfs_interface, path: &PathBuf) -> Result<(), Box<dyn Error>> {
        let cpath = CString::new(path.to_str().ok_or(StorageError::InvalidUnicodePath)?)?;
        match unsafe { vfs.mkdir.ok_or(StorageError::OperationUnavailable("mkdir"))?(cpath.as_ptr()) } {
            0 | -2 => {
                debug!("Created or using existing storage dir {path:?}");
                Ok(())
            } // Success
            error => Err(StorageError::MkdirError(error, path.clone()).into()),
        }
    }
}

impl StorageBackend for RetroVfsStorageBackend {
    fn get(&self, name: &str) -> Option<Vec<u8>> {
        assert!(!self.vfs.iface.is_null()); // Verified by the constructor

        let path = self.get_shared_object_path(name);
        if !Self::is_path_allowed(&path) {
            return None;
        }

        let vfs = unsafe { *self.vfs.iface };
        let handle = {
            let path = CString::new(path.to_str()?).ok()?;
            let handle = unsafe {
                vfs.open?(
                    path.as_ptr(),
                    VfsFileOpenFlags::READ.bits(),
                    VfsFileOpenHints::NONE.bits(),
                )
            };
            if handle.is_null() {
                error!("Failed to open {path:?}");
                return None;
            }
            // Return None if the file doesn't exist or its path is invalid
            handle
        };

        let size = unsafe {
            match vfs.size.map(|size| size(handle)) {
                None | Some(-1) => {
                    // Error, either vfs.size wasn't provided or it returned -1
                    error!("Failed to get size of {path:?}");
                    vfs.close?(handle);
                    return None;
                    // If vfs.close fails or wasn't provided, not much we can do about it
                }
                Some(size) => size,
            }
        };
        let mut buffer: Vec<u8> = Vec::with_capacity(size as usize);
        match vfs
            .read
            .map(|read| unsafe { read(handle, buffer.as_mut_ptr() as *mut _, size as u64) })
        {
            None | Some(-1) => {
                error!("Failed to read from {size}-byte file {path:?}");
                unsafe {
                    vfs.close?(handle);
                }
                return None;
                // If vfs.close fails or wasn't provided, not much we can do about it
            }
            Some(bytes_read) if bytes_read != size => {
                warn!("Expected to read {size} bytes from {path:?}, got {bytes_read}");
            }
            Some(_) => {} // Success, no action needed
        };

        match vfs.close.map(|close| unsafe { close(handle) }) {
            Some(0) => {} // Success, no action needed
            _ => {
                warn!("Failed to close file handle for {path:?}");
            }
        };

        Some(buffer)
    }

    fn put(&mut self, name: &str, value: &[u8]) -> bool {
        assert!(!self.vfs.iface.is_null()); // Verified by the constructor
        let path = self.get_shared_object_path(name);
        if !Self::is_path_allowed(&path) {
            return false;
        }

        let vfs = unsafe { *self.vfs.iface };
        if let Some(parent_dir) = path.parent().and_then(|p| Some(PathBuf::from(p))) {
            if let Err(e) = Self::ensure_storage_dir(vfs, &parent_dir) {
                return false;
            }
        }

        let handle = {
            let path = match path.to_str().and_then(|path| CString::new(path).ok()) {
                Some(path) => path,
                None => return false,
            };

            match vfs.open.map(|open| unsafe {
                open(
                    path.as_ptr(),
                    VfsFileOpenFlags::WRITE.bits(),
                    VfsFileOpenHints::NONE.bits(),
                )
            }) {
                None => {
                    error!("open operation not available");
                    return false;
                }
                Some(handle) if handle.is_null() => {
                    error!("Failed to open {path:?}");
                    return false;
                }
                Some(handle) => handle,
            }
            // Return false if the file doesn't exist or its path is invalid
        };

        match vfs
            .write
            .map(|write| unsafe { write(handle, value.as_ptr() as _, value.len() as u64) })
        {
            None => {
                error!("write operation not available");
                vfs.close.map(|close| unsafe { close(handle) });
                false
                // If vfs.close fails or wasn't provided, not much we can do about it
            }
            Some(-1) => {
                error!("Failed to write {} bytes to {path:?}", value.len());
                vfs.close.map(|close| unsafe { close(handle) });
                false
                // If vfs.close fails or wasn't provided, not much we can do about it
            }
            Some(_) => {
                match vfs.close.map(|close| unsafe { close(handle) }) {
                    Some(0) => {} // Success, no action needed
                    _ => {
                        warn!("Failed to close file handle for {path:?}");
                    }
                };
                true
            }
        }
    }

    fn remove_key(&mut self, name: &str) {
        assert!(!self.vfs.iface.is_null()); // Verified by the constructor
        let path = self.get_shared_object_path(name);
        if !Self::is_path_allowed(&path) {
            return;
        }

        let path = match path.to_str().and_then(|path| CString::new(path).ok()) {
            Some(path) => path,
            None => return,
        };

        let vfs = unsafe { *self.vfs.iface };
        match vfs.remove.map(|remove| unsafe { remove(path.as_ptr()) }) {
            Some(0) => {}
            None | Some(_) => {
                error!("Failed to remove {path:?}");
            }
        };
    }
}
