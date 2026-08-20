//! GAP-51 v1:输出块侧边栏视图。
//!
//! 过滤内联使用 `PaneBlockStore::filtered`(单一事实来源),
//! 视图将结果按最新优先以 `PAGE_SIZE` 大小分页,
//! 并渲染 `page`(限制在 `[0, total_pages)` 内)。

use iced::widget::{button, column, row, space, text, text_input};
use iced::{Color, Element, Length, Theme};

use super::data::{BlocksTabData, DirFindData, GitTabData, PortScannerTabData, RemoteFilesTabData, SurfaceFindData, ToolbeltTabData};
use crate::iced_app::Message;
use crate::output_blocks::{BlockFilter, OutputBlock};
use crate::sidebar::RightMode;

/// 最新优先的分页块大小。存储最多为每个窗格保留
/// `BLOCK_STORE_MAX = 256` 个块;按 `PAGE_SIZE = 50` 算,最多
/// 约 6 页密集活动,之后最早的块会被丢弃。
pub(crate) const PAGE_SIZE: usize = 50;

pub(crate) fn view_blocks<'a>(blocks: BlocksTabData<'a>) -> Element<'a, Message, Theme, iced::Renderer> {
    let dim = Color::from_rgba(0.55, 0.55, 0.55, 1.0);
    let mut col = column![].spacing(4);

    col = col.push(
        row![
            text("Command Blocks").size(12),
            space().width(Length::Fill),
            button(text(blocks.exit_filter.label()).size(10))
                .on_press(Message::BlockOutputCycleFilter)
                .padding([2, 6])
                .style(button::secondary),
        ]
        .spacing(4),
    );

    if !blocks.enabled {
        col = col.push(
            text("Enable [block_output].enabled in config")
                .size(10)
                .color(dim),
        );
        return col.into();
    }

    col = col.push(
        text_input("Search commands & output…", blocks.search)
            .on_input(Message::BlockOutputSearchChanged)
            .size(11)
            .padding(4),
    );

    // 单一来源过滤:`PaneBlockStore::filtered` 是唯一
    // 知晓如何按退出码 + 查询匹配 command/output_text 的地方。
    let filter = BlockFilter {
        exit: blocks.exit_filter,
        query: Some(blocks.search),
    };
    let filtered = blocks.store.filtered(&filter);
    let filtered_total = filtered.len();
    let store_total = blocks.store.len();

    if filtered_total == 0 {
        if store_total == 0 {
            col = col.push(text("(no blocks yet)").size(10).color(dim));
        } else {
            col = col.push(
                text("(no blocks match filter)").size(10).color(dim),
            );
        }
        return col.into();
    }

    let page_size = blocks.page_size.max(1);
    let total_pages = filtered_total.div_ceil(page_size);
    // 限制 page,使其始终指向有效的切片。
    let page = blocks.page.min(total_pages.saturating_sub(1));

    // filtered 为旧→新排列;page 0 = 最后的 `page_size` 条;
    // page 1 = 其前一块;以此类推。
    let start = filtered_total.saturating_sub((page + 1) * page_size);
    let end = filtered_total.saturating_sub(page * page_size);
    let page_slice: &[&OutputBlock] = &filtered[start..end];

    // 页脚:显示“第 X–Y 条,共 K 条” + 上一页/下一页按钮。
    let first = start + 1;
    let last = end;
    let footer = row![
        text(format!("{first}–{last} of {filtered_total}"))
            .size(9)
            .color(dim),
        space().width(Length::Fill),
        button(text("‹").size(10))
            .on_press_maybe((page > 0).then(|| Message::BlockOutputPageChanged {
                pane: blocks.pane_id,
                delta: -1,
            }))
            .padding([2, 6])
            .style(button::secondary),
        text(format!("{}/{}", page + 1, total_pages))
            .size(9)
            .color(dim),
        button(text("›").size(10))
            .on_press_maybe((page + 1 < total_pages).then(|| Message::BlockOutputPageChanged {
                pane: blocks.pane_id,
                delta: 1,
            }))
            .padding([2, 6])
            .style(button::secondary),
    ]
    .spacing(4);
    col = col.push(footer);

    for block in page_slice.iter().copied() {
        let cmd = block
            .command
            .as_deref()
            .unwrap_or("(shell)");
        let exit = block
            .exit_code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "?".into());
        let dur = block
            .duration_ms()
            .map(|ms| format!("{ms}ms"))
            .unwrap_or_default();
        let exit_color = if block.failed() {
            Color::from_rgb(1.0, 0.45, 0.45)
        } else {
            Color::from_rgba(0.55, 0.85, 0.65, 1.0)
        };
        let label = format!("exit {exit}  {dur}");
        col = col.push(
            button(
                column![
                    text(cmd).size(10),
                    text(label).size(9).color(exit_color),
                    if let Some(out) = block.output_text.as_deref() {
                        let preview: String = out.chars().take(120).collect();
                        text(preview).size(8).color(dim)
                    } else {
                        text("").size(8)
                    },
                ]
                .spacing(2),
            )
            .on_press(Message::BlockOutputJumpToRow(block.command_row))
            .width(Length::Fill)
            .style(button::secondary),
        );
    }

    col.into()
}
