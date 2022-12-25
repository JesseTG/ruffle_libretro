use std::error::Error;
use std::path::{Component, Path, PathBuf};
use log::{error, warn, log, debug};
use ruffle_core::backend::storage::StorageBackend;
use rust_libretro::contexts::GenericContext;
use rust_libretro::types::{VfsFileOpenFlags, VfsFileOpenHints, VfsStat};

pub struct RetroVfsStorageBackend<'a> {
    base_path: PathBuf,
    shared_objects_path: PathBuf,
    context: GenericContext<'a>,
}

impl<'a> RetroVfsStorageBackend<'a> {
    pub fn new(base_path: &Path, context: GenericContext) -> Result<Self, Box<dyn Error>> {
        let shared_objects_path = base_path.join("SharedObjects");

        return match Self::create_storage_dir(&context, &shared_objects_path) {
            Ok(_) => Ok(Self {
                base_path: PathBuf::from(base_path),
                shared_objects_path,
                context,
            }),
            Err(error) => Err(error),
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

    fn create_storage_dir(context: &GenericContext, path: &PathBuf) -> Result<(), Box<dyn Error>> {
        return match context.vfs_mkdir(path.to_str().ok_or("Save data path is invalid Unicode")?) {
            Ok(()) => {
                debug!("[ruffle] Created storage dir {path:?}");
                Ok(())
            }
            Err(e) => {
                warn!("[ruffle] Failed to create storage dir {path:?}: {e}");
                Err(e)
            }
        };
    }
}

impl<'a> StorageBackend for RetroVfsStorageBackend<'a> {
    fn get(&self, name: &str) -> Option<Vec<u8>> {
        let path = self.get_shared_object_path(name);
        if !Self::is_path_allowed(&path) {
            return None;
        }

        let mut handle = self
            .context
            .vfs_open(path.to_str()?, VfsFileOpenFlags::READ, VfsFileOpenHints::NONE)
            .ok()?;
        // Return None if the file doesn't exist or its path is invalid

        let result = match self.context.vfs_size(&mut handle) {
            Ok(size) => self.context.vfs_read(&mut handle, size as usize),
            Err(error) => Err(error),
        };

        if let Err(error) = self.context.vfs_close(handle) {
            error!("[ruffle]: Failed to close {handle:?}: {error:?}");
        }

        return result.ok();
    }

    fn put(&mut self, name: &str, value: &[u8]) -> bool {
        let path = self.get_shared_object_path(name);
        if !Self::is_path_allowed(&path) {
            return false;
        }

        if let Some(parent_dir) = path.parent().and_then(|p| Some(PathBuf::from(p))) {
            if let Err(e) = Self::create_storage_dir(&self.context, &parent_dir) {
                return false;
            }
        }

        let path = match path.to_str() {
            Some(path) => path,
            None => return false,
        };

        let mut handle = match self
            .context
            .vfs_open(path, VfsFileOpenFlags::WRITE, VfsFileOpenHints::NONE)
        {
            Ok(handle) => handle,
            Err(error) => {
                error!("[ruffle] Unable to open {path}: {error}");
                return false;
            }
        };

        // TODO: Open an issue with rust-libretro about mutability
        let mut value = value.clone();
        let success = match self.context.vfs_write(&mut handle, &mut value) {
            Ok(written) => true,
            Err(error) => {
                error!("[ruffle] Failed to write to {path}: {error}");
                false
            }
        };

        if let Err(error) = self.context.vfs_close(handle) {
            error!("[ruffle]: Failed to close {handle:?}: {error:?}");
        }

        return success;
    }

    fn remove_key(&mut self, name: &str) {
        let path = self.get_shared_object_path(name);
        if !Self::is_path_allowed(&path) {
            return;
        }

        if let Some(path) = path.to_str() {
            match self.context.vfs_remove(path) {
                Ok(_) => (),
                Err(error) => error!("[ruffle] Failed to remove {path}: {error}"),
            }
        }
    }
}
