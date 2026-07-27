//! GAP-19: Right sidebar — Find / Feed / Git mode tabs.

mod data;
mod main;
mod view_find;
mod view_feed;
mod view_toolbelt;
mod view_files;
mod view_blocks;
mod view_ports;
mod view_git;

pub(crate) use data::{
    BlocksTabData, DirFindData, GitTabData, PortScannerTabData, RemoteFilesTabData,
    SurfaceFindData, ToolbeltTabData,
};
pub(crate) use main::view;
pub(crate) use view_blocks::PAGE_SIZE;
