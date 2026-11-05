//! Port of `src/download_manager.{hpp,cpp}`.
//!
//! Downloads are first queued as pending; when accepted (by the user through
//! the control bar), they are written into a temporary directory. A completed
//! download is handed over as a [`CompletedDownload`], which deletes the file
//! when the last reference is dropped (the vice plugin holds a reference until
//! it calls the cleanup function, possibly from another thread).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::rc::Weak;
use std::sync::Arc;

use cef::*;

use crate::temp_dir::TempDir;

pub struct CompletedDownload {
    _temp_dir: Arc<TempDir>,
    path: String,
    name: String,
}

impl CompletedDownload {
    pub fn new(temp_dir: Arc<TempDir>, path: String, name: String) -> CompletedDownload {
        CompletedDownload {
            _temp_dir: temp_dir,
            path,
            name,
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Drop for CompletedDownload {
    fn drop(&mut self) {
        if std::fs::remove_file(&self.path).is_err() {
            #[cfg(not(windows))]
            crate::warning_log!("Unlinking file {} failed", self.path);
            #[cfg(windows)]
            crate::warning_log!("Deleting file {} failed", self.path);
        }
        // The temporary directory is removed when its last user is dropped
        // (the field `_temp_dir` is dropped after this).
    }
}

pub trait DownloadManagerEventHandler {
    fn on_pending_download_count_changed(&self, count: i32);
    fn on_download_progress_changed(&self, _progress: Vec<i32>) {}
    fn on_download_completed(&self, file: Arc<CompletedDownload>);
}

struct DownloadInfo {
    file_idx: i32,
    name: String,
    start_callback: Option<BeforeDownloadCallback>,
    cancel_callback: Option<DownloadItemCallback>,
    progress: i32,
}

pub struct DownloadManager {
    event_handler: Weak<dyn DownloadManagerEventHandler>,
    temp_dir: RefCell<Option<Arc<TempDir>>>,
    next_file_idx: Cell<i32>,
    infos: RefCell<BTreeMap<u32, DownloadInfo>>,
    pending: RefCell<VecDeque<u32>>,
}

impl DownloadManager {
    pub fn new(event_handler: Weak<dyn DownloadManagerEventHandler>) -> DownloadManager {
        DownloadManager {
            event_handler,
            temp_dir: RefCell::new(None),
            next_file_idx: Cell::new(1),
            infos: RefCell::new(BTreeMap::new()),
            pending: RefCell::new(VecDeque::new()),
        }
    }

    pub fn accept_pending_download(&self) {
        let id = self.pending.borrow_mut().pop_front();
        let Some(id) = id else { return };
        self.pending_download_count_changed();

        let (file_idx, callback) = {
            let mut infos = self.infos.borrow_mut();
            let Some(info) = infos.get_mut(&id) else {
                crate::panic_log!("Requirement 'infos_.count(id)' failed");
            };
            let Some(callback) = info.start_callback.take() else {
                crate::panic_log!("Requirement 'info.startCallback' failed");
            };
            (info.file_idx, callback)
        };
        let path = self.file_path(file_idx);
        callback.cont(Some(&CefString::from(path.as_str())), 0);

        self.download_progress_changed();
    }

    pub fn pending_count(&self) -> usize {
        self.pending.borrow().len()
    }

    fn temp_dir(&self) -> Arc<TempDir> {
        self.temp_dir
            .borrow_mut()
            .get_or_insert_with(|| Arc::new(TempDir::create()))
            .clone()
    }

    fn file_path(&self, file_idx: i32) -> String {
        format!("{}{}file_{}.bin", self.temp_dir().path(), crate::common::PATH_SEP, file_idx)
    }

    fn unlink_file(&self, file_idx: i32) {
        let path = self.file_path(file_idx);
        if std::fs::remove_file(&path).is_err() {
            #[cfg(not(windows))]
            crate::warning_log!("Unlinking file {} failed", path);
            #[cfg(windows)]
            crate::warning_log!("Deleting file {} failed", path);
        }
    }

    fn pending_download_count_changed(&self) {
        let count = self.pending.borrow().len() as i32;
        let handler = self.event_handler.clone();
        crate::ui::post_task(move || {
            if let Some(handler) = handler.upgrade() {
                handler.on_pending_download_count_changed(count);
            }
        });
    }

    fn download_progress_changed(&self) {
        let mut pairs: Vec<(i32, i32)> = self
            .infos
            .borrow()
            .values()
            .filter(|info| info.start_callback.is_none())
            .map(|info| (info.file_idx, info.progress))
            .collect();
        pairs.sort();
        let progress: Vec<i32> = pairs.into_iter().map(|(_, p)| p).collect();
        let handler = self.event_handler.clone();
        crate::ui::post_task(move || {
            if let Some(handler) = handler.upgrade() {
                handler.on_download_progress_changed(progress);
            }
        });
    }

    /// `DownloadHandler::OnBeforeDownload`.
    pub fn on_before_download(
        &self,
        download_item: &DownloadItem,
        suggested_name: String,
        callback: BeforeDownloadCallback,
    ) {
        if download_item.is_valid() == 0 {
            crate::panic_log!("Requirement 'downloadItem->IsValid()' failed");
        }
        let id = download_item.id();
        let file_idx = self.next_file_idx.get();
        self.next_file_idx.set(file_idx + 1);
        {
            let mut infos = self.infos.borrow_mut();
            if infos.contains_key(&id) {
                crate::panic_log!("Requirement '!downloadManager_->infos_.count(id)' failed");
            }
            infos.insert(
                id,
                DownloadInfo {
                    file_idx,
                    name: suggested_name,
                    start_callback: Some(callback),
                    cancel_callback: None,
                    progress: 0,
                },
            );
        }
        self.pending.borrow_mut().push_back(id);
        self.pending_download_count_changed();
    }

    /// `DownloadHandler::OnDownloadUpdated`.
    pub fn on_download_updated(&self, download_item: &DownloadItem, callback: Option<DownloadItemCallback>) {
        if download_item.is_valid() == 0 {
            crate::panic_log!("Requirement 'downloadItem->IsValid()' failed");
        }
        let id = download_item.id();

        enum Outcome {
            Completed(i32, String),
            Canceled(i32, Option<DownloadItemCallback>),
            Progress,
        }

        let outcome = {
            let mut infos = self.infos.borrow_mut();
            let Some(info) = infos.get_mut(&id) else { return };
            if info.start_callback.is_some() {
                return;
            }
            info.cancel_callback = callback;

            if download_item.is_complete() != 0 {
                let info = infos.remove(&id).unwrap();
                Outcome::Completed(info.file_idx, info.name)
            } else if download_item.is_in_progress() == 0 {
                let info = infos.remove(&id).unwrap();
                Outcome::Canceled(info.file_idx, info.cancel_callback)
            } else {
                let mut progress = download_item.percent_complete();
                if progress == -1 {
                    progress = 50;
                }
                info.progress = progress.clamp(0, 100);
                Outcome::Progress
            }
        };

        match outcome {
            Outcome::Completed(file_idx, name) => {
                let file = Arc::new(CompletedDownload::new(
                    self.temp_dir(),
                    self.file_path(file_idx),
                    name,
                ));
                let handler = self.event_handler.clone();
                crate::ui::post_task(move || {
                    if let Some(handler) = handler.upgrade() {
                        handler.on_download_completed(file);
                    }
                });
            }
            Outcome::Canceled(file_idx, cancel_callback) => {
                if let Some(cancel_callback) = cancel_callback {
                    cancel_callback.cancel();
                }
                self.unlink_file(file_idx);
            }
            Outcome::Progress => {}
        }
        self.download_progress_changed();
    }
}

impl Drop for DownloadManager {
    fn drop(&mut self) {
        let infos = std::mem::take(&mut *self.infos.borrow_mut());
        for info in infos.values() {
            if info.start_callback.is_none() {
                if let Some(cancel_callback) = &info.cancel_callback {
                    cancel_callback.cancel();
                }
                self.unlink_file(info.file_idx);
            }
        }
    }
}

wrap_download_handler! {
    // CEF download handler of a window; forwards to the window's
    // [`DownloadManager`] (looked up by window handle on the UI thread).
    pub struct WindowDownloadHandler {
        window_handle: u64,
    }

    impl DownloadHandler {
        fn can_download(
            &self,
            _browser: Option<&mut Browser>,
            _url: Option<&CefString>,
            _request_method: Option<&CefString>,
        ) -> ::std::os::raw::c_int {
            1
        }

        fn on_before_download(
            &self,
            _browser: Option<&mut Browser>,
            download_item: Option<&mut DownloadItem>,
            suggested_name: Option<&CefString>,
            callback: Option<&mut BeforeDownloadCallback>,
        ) -> ::std::os::raw::c_int {
            crate::ui::require_ui_thread();
            let (Some(item), Some(callback)) = (download_item, callback) else {
                return 0;
            };
            let name = suggested_name.map(|s| s.to_string()).unwrap_or_default();
            match crate::window::download_manager_of(self.window_handle) {
                Some(dm) => {
                    dm.on_before_download(item, name, callback.clone());
                    1
                }
                None => 0,
            }
        }

        fn on_download_updated(
            &self,
            _browser: Option<&mut Browser>,
            download_item: Option<&mut DownloadItem>,
            callback: Option<&mut DownloadItemCallback>,
        ) {
            crate::ui::require_ui_thread();
            let Some(item) = download_item else { return };
            if let Some(dm) = crate::window::download_manager_of(self.window_handle) {
                dm.on_download_updated(item, callback.map(|c| c.clone()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_download_deletes_file_and_temp_dir() {
        let temp_dir = Arc::new(TempDir::create());
        let dir_path = temp_dir.path().to_string();
        let path = format!("{dir_path}/file_1.bin");
        std::fs::write(&path, b"data").unwrap();

        let file = Arc::new(CompletedDownload::new(temp_dir, path.clone(), "a.txt".into()));
        assert_eq!(file.name(), "a.txt");
        // The plugin may release its reference from another thread.
        let handle = std::thread::spawn(move || drop(file));
        handle.join().unwrap();

        assert!(!std::path::Path::new(&path).exists());
        assert!(!std::path::Path::new(&dir_path).exists());
    }
}
