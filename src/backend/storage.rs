use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use ruffle_core::backend::storage::StorageBackend;
use rust_libretro::contexts::GenericContext;
use rust_libretro::environment::get_save_directory;
use rust_libretro::types::{VfsFileOpenFlags, VfsFileOpenHints};

pub struct RetroVfsStorageBackend<'a> {
    base_path: PathBuf,
    shared_objects_path: PathBuf,
    context: GenericContext<'a>,
}

impl<'a> RetroVfsStorageBackend<'a> {
    pub fn new(base_path: &Path, context: GenericContext) -> Self {
        let shared_objects_path = base_path.join("SharedObjects");

        // Create a base dir if one doesn't exist yet
        if !shared_objects_path.exists() {
            log::info!("Creating storage dir");
            if let Err(r) = fs::create_dir_all(&base_path) {
                log::warn!("Unable to create storage dir {}", r);
            }
        }


        Self {
            base_path: PathBuf::from(base_path),
            shared_objects_path,
            context,
        }
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
}

impl<'a> StorageBackend for RetroVfsStorageBackend<'a> {
    fn get(&self, name: &str) -> Option<Vec<u8>> {
        let path = self.get_shared_object_path(name);
        if !Self::is_path_allowed(&path) {
            return None;
        }

        if let Some(path) = path.to_str()
        {
            let handle = self.context.vfs_open(path, VfsFileOpenFlags::READ, VfsFileOpenHints::NONE);
        }

        None
    }

    fn put(&mut self, name: &str, value: &[u8]) -> bool {
        let path = self.get_shared_object_path(name);
        if !Self::is_path_allowed(&path) {
            return false;
        }

        if let Some(parent_dir) = path.parent().and_then(|f| f.to_str()) {
            // TODO: Create the storage directory if it doesn't already exist
            let handle = match self.context.vfs_opendir(parent_dir, true)
            {
                Ok(parent_handle) => parent_handle,
                Err(error) => {
                    log::error!("[ruffle] Unable to create storage dir {parent_dir}: {error}");
                    return false;
                }
            };

            if !parent_dir.exists() {
                if let Err(r) = fs::create_dir_all(&parent_dir) {
                    log::warn!("Unable to create storage dir {}", r);
                    return false;
                }
            }
        }

        match File::create(path) {
            Ok(mut file) => {
                if let Err(r) = file.write_all(value) {
                    log::warn!("Unable to write file content {:?}", r);
                    false
                } else {
                    true
                }
            }
            Err(r) => {
                log::warn!("Unable to save file {:?}", r);
                false
            }
        }
    }

    fn remove_key(&mut self, name: &str) {
        let path = self.get_shared_object_path(name);
        if !Self::is_path_allowed(&path) {
            return;
        }

        if let Some(path) = path.to_str() {
            match self.context.vfs_remove(path) {
                Ok(_) => (),
                Err(error) => log::error!("[ruffle] Failed to remove {path}: {error}"),
            }
        }
    }
}