//! GAP-43:远程文件浏览——经 `remote.listdir` / `remote.read` 的侧边栏 Files 标签页。

use std::path::Path;

use crate::daemon_client::{self, RemoteFileEntry, RemoteFileRead, RemoteFindMatch};
use crate::iced_app::Message;

/// `remote.read` 的预览最大字节数(与守护进程默认一致)。
pub const PREVIEW_MAX_BYTES: u64 = 65_536;

/// Files 标签页的侧边栏状态(守护进程会话的目录列表 + 预览)。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemoteFilesState {
    pub path: String,
    pub entries: Vec<RemoteFileEntry>,
    pub loading: bool,
    pub error: Option<String>,
    /// 该列表所属的会话(过期的异步结果会被忽略)。
    pub session_id: Option<String>,
    /// 侧边栏头部的人可读标签。
    pub session_label: Option<String>,
    /// 选中的文件路径(对 listdir 路径而言是相对会话 cwd 根)。
    pub preview_path: Option<String>,
    pub preview_loading: bool,
    pub preview: Option<RemoteFileRead>,
    pub preview_error: Option<String>,
    pub find_query: String,
    pub find_results: Vec<RemoteFindMatch>,
    pub find_loading: bool,
    pub find_error: Option<String>,
    fetch_gen: u64,
    read_gen: u64,
    find_gen: u64,
}

impl RemoteFilesState {
    pub fn begin_fetch(&mut self, session_id: &str, path: &str) -> u64 {
        self.session_id = Some(session_id.to_string());
        self.path = path.to_string();
        self.loading = true;
        self.error = None;
        self.fetch_gen = self.fetch_gen.saturating_add(1);
        self.fetch_gen
    }

    pub fn apply_listing(
        &mut self,
        gen: u64,
        session_id: &str,
        path: &str,
        result: Result<Vec<RemoteFileEntry>, String>,
    ) {
        if self.fetch_gen != gen || self.session_id.as_deref() != Some(session_id) {
            return;
        }
        self.loading = false;
        self.path = path.to_string();
        match result {
            Ok(entries) => {
                self.entries = sort_entries(entries);
                self.error = None;
            }
            Err(e) => {
                self.entries.clear();
                self.error = Some(e);
            }
        }
    }

    pub fn begin_read(&mut self, path: &str) -> u64 {
        self.preview_path = Some(path.to_string());
        self.preview_loading = true;
        self.preview = None;
        self.preview_error = None;
        self.read_gen = self.read_gen.saturating_add(1);
        self.read_gen
    }

    pub fn begin_write(&mut self) -> u64 {
        self.read_gen = self.read_gen.saturating_add(1);
        self.read_gen
    }

    pub fn write_gen_matches(&self, gen: u64) -> bool {
        self.read_gen == gen
    }

    pub fn apply_read(
        &mut self,
        gen: u64,
        session_id: &str,
        path: &str,
        result: Result<RemoteFileRead, String>,
    ) {
        if self.read_gen != gen || self.session_id.as_deref() != Some(session_id) {
            return;
        }
        if self.preview_path.as_deref() != Some(path) {
            return;
        }
        self.preview_loading = false;
        match result {
            Ok(read) => {
                self.preview = Some(read);
                self.preview_error = None;
            }
            Err(e) => {
                self.preview = None;
                self.preview_error = Some(e);
            }
        }
    }

    pub fn begin_find(&mut self, query: &str) -> u64 {
        self.find_query = query.to_string();
        self.find_loading = true;
        self.find_error = None;
        self.find_results.clear();
        self.find_gen = self.find_gen.saturating_add(1);
        self.find_gen
    }

    pub fn apply_find(
        &mut self,
        gen: u64,
        session_id: &str,
        query: &str,
        result: Result<Vec<RemoteFindMatch>, String>,
    ) {
        if self.find_gen != gen || self.session_id.as_deref() != Some(session_id) {
            return;
        }
        if self.find_query != query {
            return;
        }
        self.find_loading = false;
        match result {
            Ok(matches) => {
                self.find_results = matches;
                self.find_error = None;
            }
            Err(e) => {
                self.find_results.clear();
                self.find_error = Some(e);
            }
        }
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

fn sort_entries(mut entries: Vec<RemoteFileEntry>) -> Vec<RemoteFileEntry> {
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    entries
}

/// 为 `remote.listdir` 拼接目录路径与条目名。
pub fn join_dir(base: &str, name: &str) -> String {
    if base == "." || base.is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", base.trim_end_matches('/'), name)
    }
}

/// “向上”按钮的父路径(根目录时为 `None`)。
pub fn parent_dir(path: &str) -> Option<String> {
    if path == "." || path.is_empty() {
        return None;
    }
    let p = Path::new(path);
    let parent = p.parent()?;
    let s = parent.to_string_lossy();
    if s.is_empty() {
        Some(if path.starts_with('/') {
            "/".to_string()
        } else {
            ".".to_string()
        })
    } else {
        Some(s.into_owned())
    }
}

/// 在 GUI 线程之外异步执行 `remote.listdir`。
pub fn spawn_listdir(session_id: String, path: String, gen: u64) -> iced::Task<Message> {
    iced::Task::perform(
        async move {
            let sid = session_id.clone();
            let dir = path.clone();
            let result = tokio::task::spawn_blocking(move || {
                let socket = daemon_client::ensure_daemon().map_err(|e| e.to_string())?;
                daemon_client::remote_listdir(&socket, &sid, &dir).map_err(|e| e.to_string())
            })
            .await
            .unwrap_or(Err("listdir task failed".into()));
            (gen, session_id, path, result)
        },
        |(gen, session_id, path, result)| Message::RemoteFilesListed {
            gen,
            session_id,
            path,
            result,
        },
    )
}

/// 供文件预览的异步 `remote.read`。
pub fn spawn_read(
    session_id: String,
    path: String,
    gen: u64,
) -> iced::Task<Message> {
    iced::Task::perform(
        async move {
            let sid = session_id.clone();
            let file_path = path.clone();
            let result = tokio::task::spawn_blocking(move || {
                let socket = daemon_client::ensure_daemon().map_err(|e| e.to_string())?;
                daemon_client::remote_read(&socket, &sid, &file_path, PREVIEW_MAX_BYTES)
                    .map_err(|e| e.to_string())
            })
            .await
            .unwrap_or(Err("read task failed".into()));
            (gen, session_id, path, result)
        },
        |(gen, session_id, path, result)| Message::RemoteFilesRead {
            gen,
            session_id,
            path,
            result,
        },
    )
}

/// 供文件名搜索的异步 `remote.find`。
pub fn spawn_find(
    session_id: String,
    base: String,
    query: String,
    gen: u64,
) -> iced::Task<Message> {
    iced::Task::perform(
        async move {
            let sid = session_id.clone();
            let q = query.clone();
            let base_path = base.clone();
            let result = tokio::task::spawn_blocking(move || {
                let socket = daemon_client::ensure_daemon().map_err(|e| e.to_string())?;
                daemon_client::remote_find(&socket, &sid, &base_path, &q).map_err(|e| e.to_string())
            })
            .await
            .unwrap_or(Err("find task failed".into()));
            (gen, session_id, query, result)
        },
        |(gen, session_id, query, result)| Message::RemoteFilesFind {
            gen,
            session_id,
            query,
            result,
        },
    )
}

/// 供保存预览编辑的异步 `remote.write`。
pub fn spawn_write(
    session_id: String,
    path: String,
    content: String,
    gen: u64,
) -> iced::Task<Message> {
    iced::Task::perform(
        async move {
            let sid = session_id.clone();
            let p = path.clone();
            let result = tokio::task::spawn_blocking(move || {
                let socket = daemon_client::ensure_daemon().map_err(|e| e.to_string())?;
                daemon_client::remote_write(&socket, &sid, &p, &content).map_err(|e| e.to_string())
            })
            .await
            .unwrap_or(Err("write task failed".into()));
            (gen, session_id, path, result)
        },
        |(gen, session_id, path, result)| Message::RemoteFilesWritten {
            gen,
            session_id,
            path,
            result,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_dir_from_dot() {
        assert_eq!(join_dir(".", "src"), "src");
        assert_eq!(join_dir("", "etc"), "etc");
    }

    #[test]
    fn join_dir_nested() {
        assert_eq!(join_dir("home/user", "docs"), "home/user/docs");
        assert_eq!(join_dir("/var/log/", "syslog"), "/var/log/syslog");
    }

    #[test]
    fn parent_dir_at_root() {
        assert_eq!(parent_dir("."), None);
        assert_eq!(parent_dir(""), None);
    }

    #[test]
    fn parent_dir_nested() {
        assert_eq!(parent_dir("a/b"), Some("a".into()));
        assert_eq!(parent_dir("/var/log/syslog"), Some("/var/log".into()));
    }

    #[test]
    fn apply_listing_ignores_stale_gen() {
        let mut st = RemoteFilesState::default();
        let gen = st.begin_fetch("sess-1", ".");
        st.apply_listing(
            gen + 1,
            "sess-1",
            ".",
            Ok(vec![RemoteFileEntry {
                name: "stale".into(),
                is_dir: false,
                size: None,
            }]),
        );
        assert!(st.loading);
        assert!(st.entries.is_empty());
    }

    #[test]
    fn apply_read_ignores_stale_gen() {
        let mut st = RemoteFilesState::default();
        st.session_id = Some("sess-1".into());
        let gen = st.begin_read("a.txt");
        st.apply_read(
            gen + 1,
            "sess-1",
            "a.txt",
            Ok(RemoteFileRead {
                path: "a.txt".into(),
                content: "hi".into(),
                truncated: false,
                size: Some(2),
            }),
        );
        assert!(st.preview_loading);
        assert!(st.preview.is_none());
    }

    #[test]
    fn sort_entries_dirs_first() {
        let rows = sort_entries(vec![
            RemoteFileEntry {
                name: "b.txt".into(),
                is_dir: false,
                size: None,
            },
            RemoteFileEntry {
                name: "a".into(),
                is_dir: true,
                size: None,
            },
        ]);
        assert!(rows[0].is_dir);
        assert_eq!(rows[1].name, "b.txt");
    }
}
