#!/usr/bin/env python3
"""Migrate docs/featuresList/*.md → docs/features/GAP-XX-slug/spec.md with frontmatter."""

from __future__ import annotations

import re
import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "docs" / "featuresList"
DST = ROOT / "docs" / "features"

# file_stem (without .md) → metadata
# dir naming per plan; gap id for frontmatter
META: dict[str, dict] = {
    "01-ai-chat-panel": {
        "gap": "GAP-01", "dir": "GAP-01-ai-chat-panel", "title": "AI Chat Panel",
        "category": "AI Agent", "plan_type": "feature", "priority": "P0", "effort": "XL",
        "status": "in-progress", "closed": None,
    },
    "02-error-recovery": {
        "gap": "GAP-02", "dir": "GAP-02-error-recovery", "title": "Error Recovery",
        "category": "AI Agent", "plan_type": "feature", "priority": "P0", "effort": "M",
        "status": "not-started", "closed": None,
    },
    "03-codebase-indexing": {
        "gap": "GAP-03", "dir": "GAP-03-codebase-indexing", "title": "Codebase Indexing",
        "category": "AI Agent", "plan_type": "feature", "priority": "P1", "effort": "L",
        "status": "not-started", "closed": None,
    },
    "04-computer-use": {
        "gap": "GAP-04", "dir": "GAP-04-computer-use", "title": "Computer Use",
        "category": "AI Agent", "plan_type": "feature", "priority": "P2", "effort": "XL",
        "status": "not-started", "closed": None,
    },
    "05-voice-input": {
        "gap": "GAP-05", "dir": "GAP-05-voice-input", "title": "Voice Input",
        "category": "AI Agent", "plan_type": "feature", "priority": "P4", "effort": "L",
        "status": "not-started", "closed": None,
    },
    "06-ambient-agents": {
        "gap": "GAP-06", "dir": "GAP-06-ambient-agents", "title": "Ambient Agents",
        "category": "AI Agent", "plan_type": "feature", "priority": "P3", "effort": "XL",
        "status": "not-started", "closed": None,
    },
    "07-soul-system": {
        "gap": "GAP-07", "dir": "GAP-07-soul-system", "title": "Soul System",
        "category": "AI Agent", "plan_type": "feature", "priority": "P2", "effort": "S",
        "status": "not-started", "closed": None,
    },
    "08-natural-language-commands": {
        "gap": "GAP-08", "dir": "GAP-08-natural-language-commands", "title": "Natural Language Commands",
        "category": "AI Agent", "plan_type": "feature", "priority": "P2", "effort": "M",
        "status": "not-started", "closed": None,
    },
    "09-notification-system": {
        "gap": "GAP-09", "dir": "GAP-09-notification", "title": "Notification System",
        "category": "终端交互", "plan_type": "wrap-up", "priority": "P0", "effort": "M",
        "status": "in-progress", "closed": None, "e2e": "manual-e2e/gap-09-notification.md",
    },
    "10-triggers": {
        "gap": "GAP-10", "dir": "GAP-10-triggers", "title": "Triggers",
        "category": "终端交互", "plan_type": "feature", "priority": "P1", "effort": "L",
        "status": "in-progress", "closed": None,
        "plan": "../../superpowers/plans/2026-05-15-triggers-system.md",
    },
    "11-instant-replay": {
        "gap": "GAP-11", "dir": "GAP-11-instant-replay", "title": "Instant Replay",
        "category": "终端交互", "plan_type": "feature", "priority": "P2", "effort": "L",
        "status": "not-started", "closed": None,
    },
    "12-annotations-marks": {
        "gap": "GAP-12", "dir": "GAP-12-annotations-marks", "title": "Annotations & Marks",
        "category": "终端交互", "plan_type": "feature", "priority": "P2", "effort": "M",
        "status": "not-started", "closed": None,
    },
    "13-broadcast-input": {
        "gap": "GAP-13", "dir": "GAP-13-broadcast-input", "title": "Broadcast Input",
        "category": "终端交互", "plan_type": "feature", "priority": "P3", "effort": "S",
        "status": "not-started", "closed": None,
    },
    "14-enhanced-vim-mode": {
        "gap": "GAP-14", "dir": "GAP-14-enhanced-vim-mode", "title": "Enhanced Vim Mode",
        "category": "终端交互", "plan_type": "enhance", "priority": "P3", "effort": "M",
        "status": "not-started", "closed": None,
    },
    "15-shell-integration": {
        "gap": "GAP-15", "dir": "GAP-15-shell-integration", "title": "Shell Integration",
        "category": "终端交互", "plan_type": "wrap-up", "priority": "P0", "effort": "L",
        "status": "in-progress", "closed": None,
    },
    "16-hover-preview": {
        "gap": "GAP-4.2", "dir": "GAP-4.2-hover-preview", "title": "Hover Preview",
        "category": "终端交互", "plan_type": "enhance", "priority": "P2", "effort": "M",
        "status": "in-progress", "closed": None,
    },
    "17-sixel-graphics-protocol": {
        "gap": "GAP-17", "dir": "GAP-17-sixel-graphics-protocol", "title": "Sixel & Chunked Graphics",
        "category": "终端交互", "plan_type": "feature", "priority": "P4", "effort": "L",
        "status": "not-started", "closed": None,
    },
    "18-bidi-support": {
        "gap": "GAP-18", "dir": "GAP-18-bidi-support", "title": "BiDi Support",
        "category": "终端交互", "plan_type": "feature", "priority": "P3", "effort": "L",
        "status": "not-started", "closed": None,
    },
    "19-dual-sidebar": {
        "gap": "GAP-19", "dir": "GAP-19-dual-sidebar", "title": "Dual Sidebar",
        "category": "UI/外壳", "plan_type": "feature", "priority": "P1", "effort": "L",
        "status": "not-started", "closed": None,
    },
    "20-glass-effect": {
        "gap": "GAP-20", "dir": "GAP-20-glass-effect", "title": "Glass Effect",
        "category": "UI/外壳", "plan_type": "platform", "priority": "P4", "effort": "M",
        "status": "not-started", "closed": None,
    },
    "21-dock-badge": {
        "gap": "GAP-21", "dir": "GAP-21-dock-badge", "title": "Dock Badge",
        "category": "UI/外壳", "plan_type": "wrap-up", "priority": "P4", "effort": "S",
        "status": "in-progress", "closed": None,
    },
    "22-touch-bar": {
        "gap": "GAP-22", "dir": "GAP-22-touch-bar", "title": "Touch Bar",
        "category": "UI/外壳", "plan_type": "platform", "priority": "P4", "effort": "M",
        "status": "not-started", "closed": None,
    },
    "23-tab-bar": {
        "gap": "GAP-23", "dir": "GAP-23-tab-bar", "title": "Tab Bar",
        "category": "UI/外壳", "plan_type": "feature", "priority": "P4", "effort": "M",
        "status": "not-started", "closed": None,
    },
    "24-status-bar-enhanced": {
        "gap": "GAP-24", "dir": "GAP-24-status-bar", "title": "Enhanced Status Bar",
        "category": "UI/外壳", "plan_type": "enhance", "priority": "P2", "effort": "L",
        "status": "done", "closed": "2026-06-26",
        "e2e": "manual-e2e/gap-32-git-integration.md",
    },
    "25-multi-window": {
        "gap": "GAP-25", "dir": "GAP-25-multi-window", "title": "Multi-Window",
        "category": "UI/外壳", "plan_type": "feature", "priority": "P2", "effort": "L",
        "status": "not-started", "closed": None,
    },
    "26-workspace-groups": {
        "gap": "GAP-26", "dir": "GAP-26-workspace-groups", "title": "Workspace Groups",
        "category": "UI/外壳", "plan_type": "feature", "priority": "P3", "effort": "S",
        "status": "not-started", "closed": None,
    },
    "27-settings-panel": {
        "gap": "GAP-27", "dir": "GAP-27-settings-panel", "title": "Settings Panel",
        "category": "UI/外壳", "plan_type": "feature", "priority": "P4", "effort": "M",
        "status": "done", "closed": "2026-06-08",
    },
    "28-window-border": {
        "gap": "GAP-28", "dir": "GAP-28-window-border", "title": "Window Border",
        "category": "UI/外壳", "plan_type": "feature", "priority": "P4", "effort": "S",
        "status": "not-started", "closed": None,
    },
    "29-badge-overlay": {
        "gap": "GAP-29", "dir": "GAP-29-badge-overlay", "title": "Badge Overlay",
        "category": "UI/外壳", "plan_type": "feature", "priority": "P4", "effort": "S",
        "status": "not-started", "closed": None,
    },
    "30-toolbelt": {
        "gap": "GAP-30", "dir": "GAP-30-toolbelt", "title": "Toolbelt",
        "category": "UI/外壳", "plan_type": "feature", "priority": "P3", "effort": "L",
        "status": "not-started", "closed": None,
    },
    "31-quake-dock": {
        "gap": "GAP-31", "dir": "GAP-31-quake-dock", "title": "Quake-style Dock",
        "category": "UI/外壳", "plan_type": "feature", "priority": "P3", "effort": "M",
        "status": "not-started", "closed": None,
    },
    "32-git-integration": {
        "gap": "GAP-32", "dir": "GAP-32-git-integration", "title": "Git Integration",
        "category": "开发者工具", "plan_type": "integration", "priority": "P2", "effort": "M",
        "status": "done", "closed": "2026-06-27",
        "e2e": "manual-e2e/gap-32-git-integration.md",
    },
    "33-port-scanner": {
        "gap": "GAP-33", "dir": "GAP-33-port-scanner", "title": "Port Scanner",
        "category": "开发者工具", "plan_type": "feature", "priority": "P2", "effort": "S",
        "status": "not-started", "closed": None,
    },
    "34-file-preview": {
        "gap": "GAP-34", "dir": "GAP-34-file-preview", "title": "File Preview",
        "category": "开发者工具", "plan_type": "feature", "priority": "P4", "effort": "M",
        "status": "not-started", "closed": None,
    },
    "35-lsp-client": {
        "gap": "GAP-35", "dir": "GAP-35-lsp-client", "title": "LSP Client",
        "category": "开发者工具", "plan_type": "integration", "priority": "P1", "effort": "XL",
        "status": "not-started", "closed": None,
    },
    "36-semantic-history": {
        "gap": "GAP-36", "dir": "GAP-36-semantic-history", "title": "Semantic History",
        "category": "开发者工具", "plan_type": "feature", "priority": "P2", "effort": "M",
        "status": "not-started", "closed": None,
    },
    "37-password-manager": {
        "gap": "GAP-37", "dir": "GAP-37-password-manager", "title": "Password Manager",
        "category": "开发者工具", "plan_type": "integration", "priority": "P3", "effort": "M",
        "status": "not-started", "closed": None,
    },
    "38-file-transfer": {
        "gap": "GAP-38", "dir": "GAP-38-file-transfer", "title": "File Transfer",
        "category": "SSH/网络", "plan_type": "feature", "priority": "P4", "effort": "L",
        "status": "not-started", "closed": None,
    },
    "40-ssh-forwarding": {
        "gap": "GAP-40", "dir": "GAP-40-ssh-forwarding", "title": "SSH Forwarding",
        "category": "SSH/网络", "plan_type": "integration", "priority": "P2", "effort": "M",
        "status": "not-started", "closed": None,
        "aliases": ["GAP-5.4"],
    },
    "41-jump-host": {
        "gap": "GAP-41", "dir": "GAP-41-jump-host", "title": "Jump Host",
        "category": "SSH/网络", "plan_type": "feature", "priority": "P3", "effort": "M",
        "status": "not-started", "closed": None,
    },
    "42-login-scripts": {
        "gap": "GAP-42", "dir": "GAP-42-login-scripts", "title": "Login Scripts",
        "category": "SSH/网络", "plan_type": "feature", "priority": "P3", "effort": "S",
        "status": "not-started", "closed": None,
    },
    "43-remote-server": {
        "gap": "GAP-43", "dir": "GAP-43-remote-server", "title": "Remote Server",
        "category": "SSH/网络", "plan_type": "feature", "priority": "P2", "effort": "XL",
        "status": "not-started", "closed": None,
    },
    "44-shared-sessions": {
        "gap": "GAP-44", "dir": "GAP-44-shared-sessions", "title": "Shared Sessions",
        "category": "SSH/网络", "plan_type": "feature", "priority": "P4", "effort": "XL",
        "status": "not-started", "closed": None,
    },
    "45-auto-update": {
        "gap": "GAP-45", "dir": "GAP-45-auto-update", "title": "Auto Update",
        "category": "平台", "plan_type": "platform", "priority": "P4", "effort": "M",
        "status": "not-started", "closed": None,
    },
    "46-i18n": {
        "gap": "GAP-46", "dir": "GAP-46-i18n", "title": "i18n",
        "category": "平台", "plan_type": "feature", "priority": "P3", "effort": "L",
        "status": "not-started", "closed": None,
    },
    "47-apple-script": {
        "gap": "GAP-47", "dir": "GAP-47-apple-script", "title": "AppleScript Bridge",
        "category": "平台", "plan_type": "platform", "priority": "P2", "effort": "M",
        "status": "not-started", "closed": None,
    },
    "48-url-scheme": {
        "gap": "GAP-48", "dir": "GAP-48-url-scheme", "title": "URL Scheme",
        "category": "平台", "plan_type": "platform", "priority": "P2", "effort": "M",
        "status": "not-started", "closed": None,
    },
    "49-plugin-system": {
        "gap": "GAP-49", "dir": "GAP-49-plugin-system", "title": "Plugin System",
        "category": "集成", "plan_type": "arch", "priority": "P4", "effort": "XL",
        "status": "not-started", "closed": None,
    },
    "50-cloud-sync": {
        "gap": "GAP-50", "dir": "GAP-50-cloud-sync", "title": "Cloud Sync",
        "category": "集成", "plan_type": "integration", "priority": "P4", "effort": "M",
        "status": "not-started", "closed": None,
    },
    "51-block-based-output": {
        "gap": "GAP-51", "dir": "GAP-51-block-based-output", "title": "Block-based Output",
        "category": "集成", "plan_type": "arch", "priority": "P2", "effort": "XXL",
        "status": "not-started", "closed": None,
    },
    "52-workflows": {
        "gap": "GAP-52", "dir": "GAP-52-workflows", "title": "Workflows",
        "category": "集成", "plan_type": "feature", "priority": "P4", "effort": "M",
        "status": "not-started", "closed": None,
    },
    "53-notebooks": {
        "gap": "GAP-53", "dir": "GAP-53-notebooks", "title": "Notebooks",
        "category": "集成", "plan_type": "feature", "priority": "P4", "effort": "L",
        "status": "not-started", "closed": None,
    },
    "54-background-image": {
        "gap": "GAP-54", "dir": "GAP-54-background-image", "title": "Background Image",
        "category": "视觉", "plan_type": "feature", "priority": "P4", "effort": "M",
        "status": "not-started", "closed": None,
    },
    "55-transparency-blur": {
        "gap": "GAP-55", "dir": "GAP-55-transparency-blur", "title": "Transparency & Blur",
        "category": "视觉", "plan_type": "feature", "priority": "P4", "effort": "M",
        "status": "not-started", "closed": None,
    },
    "56-focus-follows-mouse": {
        "gap": "GAP-56", "dir": "GAP-56-focus-follows-mouse", "title": "Focus Follows Mouse",
        "category": "视觉", "plan_type": "feature", "priority": "P4", "effort": "S",
        "status": "not-started", "closed": None,
    },
    "57-haptic-feedback": {
        "gap": "GAP-57", "dir": "GAP-57-haptic-feedback", "title": "Haptic Feedback",
        "category": "视觉", "plan_type": "platform", "priority": "P4", "effort": "S",
        "status": "not-started", "closed": None,
    },
    "58-custom-theme-css": {
        "gap": "GAP-58", "dir": "GAP-58-custom-theme-css", "title": "Custom Theme & CSS",
        "category": "视觉", "plan_type": "feature", "priority": "P4", "effort": "M",
        "status": "not-started", "closed": None,
    },
    "59-emoji-rendering": {
        "gap": "GAP-59", "dir": "GAP-59-emoji-rendering", "title": "Emoji Rendering",
        "category": "视觉", "plan_type": "enhance", "priority": "P4", "effort": "M",
        "status": "not-started", "closed": None,
    },
    "60-screen-capture": {
        "gap": "GAP-60", "dir": "GAP-60-screen-capture", "title": "Screen Capture",
        "category": "视觉", "plan_type": "feature", "priority": "P4", "effort": "M",
        "status": "not-started", "closed": None,
    },
    "61-print-support": {
        "gap": "GAP-61", "dir": "GAP-61-print-support", "title": "Print Support",
        "category": "视觉", "plan_type": "feature", "priority": "P4", "effort": "S",
        "status": "not-started", "closed": None,
    },
    "62-programmable-sidebar": {
        "gap": "GAP-3.2", "dir": "GAP-3.2-programmable-sidebar", "title": "可编程 Sidebar",
        "category": "UI/外壳", "plan_type": "feature", "priority": "P2", "effort": "L",
        "status": "not-started", "closed": None,
    },
    "63-pty-resize": {
        "gap": "GAP-5.3", "dir": "GAP-5.3-pty-resize", "title": "PTY Resize",
        "category": "SSH/网络", "plan_type": "feature", "priority": "P2", "effort": "S",
        "status": "not-started", "closed": None,
    },
    "64-decscl": {
        "gap": "GAP-1.2", "dir": "GAP-1.2-decscl", "title": "DECSCL",
        "category": "终端交互", "plan_type": "feature", "priority": "P3", "effort": "M",
        "status": "not-started", "closed": None,
    },
    "65-accessibility": {
        "gap": "GAP-4.6", "dir": "GAP-4.6-accessibility", "title": "Accessibility",
        "category": "视觉", "plan_type": "platform", "priority": "P4", "effort": "M",
        "status": "not-started", "closed": None,
    },
    "66-event-stream": {
        "gap": "GAP-6.7", "dir": "GAP-6.7-event-stream", "title": "Event Stream",
        "category": "集成", "plan_type": "integration", "priority": "P0", "effort": "S",
        "status": "done", "closed": "2026-06-26",
        "e2e": "manual-e2e/gap-6.7-event-stream.md",
    },
    "67-browser-panel": {
        "gap": "GAP-3.5", "dir": "GAP-3.5-browser-panel", "title": "Browser Panel",
        "category": "UI/外壳", "plan_type": "feature", "priority": "P1", "effort": "L",
        "status": "done", "closed": "2026-06-25",
        "aliases": ["GAP-7.8"],
        "e2e": "manual-e2e/gap-3.5-browser.md",
    },
    "68-ssh-reconnect": {
        "gap": "GAP-5.6", "dir": "GAP-5.6-ssh-reconnect", "title": "SSH Reconnect",
        "category": "SSH/网络", "plan_type": "integration", "priority": "P0", "effort": "S",
        "status": "done", "closed": "2026-06-25",
        "e2e": "manual-e2e/gap-5.6-ssh-reconnect.md",
    },
    "69-socks5-tunnel": {
        "gap": "GAP-5.2", "dir": "GAP-5.2-socks5-tunnel", "title": "SOCKS5 Tunnel",
        "category": "SSH/网络", "plan_type": "integration", "priority": "P1", "effort": "M",
        "status": "done", "closed": "2026-06-27",
        "e2e": "manual-e2e/gap-5.2-socks5-tunnel.md",
    },
    "39-serial-telnet": {
        "gap": "GAP-39", "dir": "GAP-39-serial-telnet", "title": "Serial & Telnet",
        "category": "SSH/网络", "plan_type": "feature", "priority": "P3", "effort": "L",
        "status": "not-started", "closed": None,
    },
}

# stem → target dir for cross-link rewrites
STEM_TO_DIR: dict[str, str] = {k: v["dir"] for k, v in META.items()}


def yaml_frontmatter(m: dict) -> str:
    lines = ["---"]
    lines.append(f"gap: {m['gap']}")
    lines.append(f"title: {m['title']}")
    lines.append(f"category: {m['category']}")
    lines.append(f"plan_type: {m['plan_type']}")
    lines.append(f"priority: {m['priority']}")
    lines.append(f"effort: {m['effort']}")
    lines.append(f"status: {m['status']}")
    closed = m.get("closed")
    lines.append(f"closed: {closed if closed else 'null'}")
    if aliases := m.get("aliases"):
        lines.append(f"aliases: [{', '.join(aliases)}]")
    if e2e := m.get("e2e"):
        lines.append(f"e2e: {e2e}")
    if plan := m.get("plan"):
        lines.append(f"plan: {plan}")
    lines.append("---")
    return "\n".join(lines) + "\n\n"


def fix_links(content: str) -> str:
    # featuresList/NN-slug.md → ../GAP-XX-slug/spec.md
    def repl_featureslist(match: re.Match) -> str:
        stem = match.group(1)
        if stem in STEM_TO_DIR:
            return f"../{STEM_TO_DIR[stem]}/spec.md"
        return match.group(0)

    content = re.sub(
        r"featuresList/(\d{2}-[\w-]+)\.md",
        repl_featureslist,
        content,
    )
    # bare references like (09-notification-system.md)
    def repl_bare(match: re.Match) -> str:
        stem = match.group(1)
        if stem in STEM_TO_DIR:
            return f"](../{STEM_TO_DIR[stem]}/spec.md"
        return match.group(0)

    content = re.sub(
        r"\]\((\d{2}-[\w-]+)\.md\)",
        repl_bare,
        content,
    )
    # GAP-09 Notification System style links
    content = content.replace(
        "[GAP-09 Notification System](09-notification-system.md)",
        "[GAP-09 Notification System](../GAP-09-notification/spec.md)",
    )
    # manual-e2e paths from features dir: ../manual-e2e/...
    content = re.sub(
        r"\[`docs/manual-e2e/",
        r"[`../../manual-e2e/",
        content,
    )
    content = re.sub(
        r"`docs/manual-e2e/",
        r"`../../manual-e2e/",
        content,
    )
    content = re.sub(
        r"\(\.\./manual-e2e/",
        r"(../../manual-e2e/",
        content,
    )
    content = re.sub(
        r"\[(`docs/manual-e2e/[^`]+`)\]\(\.\./manual-e2e/",
        r"[\1](../../manual-e2e/",
        content,
    )
    return content


def migrate_file(stem: str) -> None:
    src_path = SRC / f"{stem}.md"
    if stem not in META:
        raise KeyError(f"Missing metadata for {stem}")
    m = META[stem]
    dest_dir = DST / m["dir"]
    dest_dir.mkdir(parents=True, exist_ok=True)
    body = src_path.read_text(encoding="utf-8")
    body = fix_links(body)
    (dest_dir / "spec.md").write_text(yaml_frontmatter(m) + body, encoding="utf-8")
    print(f"  {stem} → {m['dir']}/spec.md")


def main() -> None:
    if DST.exists():
        shutil.rmtree(DST)
    DST.mkdir(parents=True)
    for stem in sorted(META.keys()):
        migrate_file(stem)
    print(f"\nMigrated {len(META)} specs to {DST}")


if __name__ == "__main__":
    main()
