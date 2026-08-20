//! `split_tree` 的单元测试。
//!
//! 覆盖 GAP-2.2 嵌套分屏补齐(equalize/swap/neighbor)、GAP-6.6
//! swap_by_ids/detach_leaf(swap-pane/join-pane)、GAP-5.6 跨重启
//! id 重映射、resize_ratio/find_adjacent 几何邻居,以及 Surface
//! 类型别名(GAP-2.1)。

use super::*;

/// 构造 3 层嵌套:H[ V[a, b], H[c, V[d, e]] ]
/// 形成 5 个 pane(a..e),用于覆盖嵌套场景。
fn build_nested() -> (SplitNode, [PaneId; 5]) {
    let ids = [
        PaneId(101), PaneId(102), PaneId(103), PaneId(104), PaneId(105),
    ];
    let inner_v1 = SplitNode::Split {
        split_id: 1,
        direction: SplitDirection::Vertical,
        ratio: 0.3,
        first: Box::new(SplitNode::Leaf(ids[0])),
        second: Box::new(SplitNode::Leaf(ids[1])),
    };
    let inner_v2 = SplitNode::Split {
        split_id: 2,
        direction: SplitDirection::Vertical,
        ratio: 0.4,
        first: Box::new(SplitNode::Leaf(ids[3])),
        second: Box::new(SplitNode::Leaf(ids[4])),
    };
    let inner_h = SplitNode::Split {
        split_id: 3,
        direction: SplitDirection::Horizontal,
        ratio: 0.7,
        first: Box::new(SplitNode::Leaf(ids[2])),
        second: Box::new(inner_v2),
    };
    let root = SplitNode::Split {
        split_id: 4,
        direction: SplitDirection::Horizontal,
        ratio: 0.2,
        first: Box::new(inner_v1),
        second: Box::new(inner_h),
    };
    (root, ids)
}

/// 收集所有 Split 节点的 ratio,用于 equalize 断言。
fn collect_ratios(node: &SplitNode, out: &mut Vec<f32>) {
    if let SplitNode::Split { ratio, first, second, .. } = node {
        out.push(*ratio);
        collect_ratios(first, out);
        collect_ratios(second, out);
    }
}

#[test]
fn equalize_all_sets_every_split_to_half() {
    let (mut tree, _) = build_nested();
    tree.equalize_all();
    let mut ratios = Vec::new();
    collect_ratios(&tree, &mut ratios);
    assert_eq!(ratios.len(), 4, "应有 4 个 Split 节点");
    for r in ratios {
        assert!((r - 0.5).abs() < 1e-6, "每个 Split.ratio 应为 0.5,实际 {}", r);
    }
}

#[test]
fn equalize_all_on_leaf_is_noop() {
    let mut tree = SplitNode::Leaf(PaneId(42));
    tree.equalize_all();
    assert!(matches!(tree, SplitNode::Leaf(PaneId(42))));
}

// ── GAP-6.6: swap_by_ids / detach_leaf(按 id 交换 / 摘除叶子;swap-pane / join-pane) ──

#[test]
fn swap_by_ids_swaps_two_leaves_anywhere() {
    let (mut tree, ids) = build_nested(); // DFS 顺序 [a, b, c, d, e]
    assert!(tree.swap_by_ids(ids[0], ids[3])); // 交换 a ↔ d
    assert_eq!(tree.pane_ids(), vec![ids[3], ids[1], ids[2], ids[0], ids[4]]);
}

#[test]
fn swap_by_ids_twice_restores() {
    let (mut tree, ids) = build_nested();
    let before = tree.pane_ids();
    assert!(tree.swap_by_ids(ids[0], ids[4]));
    assert!(tree.swap_by_ids(ids[0], ids[4]));
    assert_eq!(tree.pane_ids(), before);
}

#[test]
fn swap_by_ids_unknown_leaves_tree_unchanged() {
    let (mut tree, ids) = build_nested();
    let before = tree.pane_ids();
    assert!(!tree.swap_by_ids(ids[0], PaneId(999)));
    assert_eq!(tree.pane_ids(), before);
}

#[test]
fn swap_by_ids_same_id_present_is_noop() {
    let (mut tree, ids) = build_nested();
    let before = tree.pane_ids();
    assert!(tree.swap_by_ids(ids[0], ids[0]));
    assert_eq!(tree.pane_ids(), before);
}

#[test]
fn detach_leaf_removes_and_keeps_rest() {
    let (mut tree, ids) = build_nested(); // 5 个窗格
    assert!(tree.detach_leaf(ids[1])); // 移除 b
    assert_eq!(tree.pane_count(), 4);
    assert!(!tree.contains(ids[1]));
    assert!(tree.contains(ids[0]));
}

#[test]
fn detach_only_leaf_fails() {
    let mut tree = SplitNode::single(PaneId(7));
    assert!(!tree.detach_leaf(PaneId(7)));
    assert!(matches!(tree, SplitNode::Leaf(PaneId(7))));
}

#[test]
fn insert_existing_pane_as_sibling_via_split() {
    // join-pane 目标侧:`split(at_leaf, existing_pane, dir)` 把外部窗格
    // 作为 at_leaf 的兄弟插入——split 本身已支持这一点。
    let (mut tree, ids) = build_nested();
    let external = PaneId(999);
    assert!(tree.split(ids[2], external, SplitDirection::Vertical));
    assert!(tree.contains(external));
    assert_eq!(tree.pane_count(), 6);
}

#[test]
fn swap_with_sibling_swaps_and_inverts_ratio() {
    let (mut tree, ids) = build_nested();
    // 找到含 ids[0] 的最浅 Split(根 H,first=V[a,b], second=H[...])
    // 期望:根 Split 的 first/second 交换,ratio 从 0.2 → 0.8
    let ok = tree.swap_with_sibling(ids[0]);
    assert!(ok);
    if let SplitNode::Split { ratio, first, .. } = &tree {
        assert!((*ratio - 0.8).abs() < 1e-6, "ratio 应翻转为 0.8,实际 {}", ratio);
        // 交换后 first 应是原 inner_h(含 ids[2..])
        assert!(first.contains(ids[2]));
        assert!(!first.contains(ids[0]));
    } else {
        panic!("根应仍为 Split");
    }
}

#[test]
fn swap_with_sibling_twice_returns_to_original() {
    let (mut tree, ids) = build_nested();
    let r1 = if let SplitNode::Split { ratio, .. } = &tree { *ratio } else { panic!() };
    assert!(tree.swap_with_sibling(ids[0]));
    assert!(tree.swap_with_sibling(ids[0]));
    let r2 = if let SplitNode::Split { ratio, .. } = &tree { *ratio } else { panic!() };
    assert!((r1 - r2).abs() < 1e-6, "swap 两次应回到原 ratio");
    // 结构也回到原状(first 仍含 ids[0])
    if let SplitNode::Split { first, .. } = &tree {
        assert!(first.contains(ids[0]));
    }
}

#[test]
fn swap_with_sibling_on_root_leaf_returns_false() {
    let pane = PaneId(7);
    let mut tree = SplitNode::Leaf(pane);
    assert!(!tree.swap_with_sibling(pane));
}

#[test]
fn swap_with_sibling_on_unknown_pane_returns_false() {
    let (mut tree, _) = build_nested();
    let unknown = PaneId(9999);
    assert!(!tree.swap_with_sibling(unknown));
}

#[test]
fn neighbor_in_order_next_wraps_at_boundary() {
    let (tree, ids) = build_nested();
    // pane_ids() DFS 先序:首先 inner_v1 (ids[0], ids[1]),
    // 然后 inner_h: ids[2], 然后 inner_v2 (ids[3], ids[4])
    let order = tree.pane_ids();
    assert_eq!(order, vec![ids[0], ids[1], ids[2], ids[3], ids[4]]);
    // 从最后一个向后移动,回绕到第一个
    assert_eq!(tree.neighbor_in_order(ids[4], 1), Some(ids[0]));
    // 从第一个向前移动,回绕到最后一个
    assert_eq!(tree.neighbor_in_order(ids[0], -1), Some(ids[4]));
    // 中间 +1
    assert_eq!(tree.neighbor_in_order(ids[2], 1), Some(ids[3]));
    // 中间 -1
    assert_eq!(tree.neighbor_in_order(ids[2], -1), Some(ids[1]));
}

#[test]
fn neighbor_in_order_unknown_pane_returns_none() {
    let (tree, _) = build_nested();
    assert_eq!(tree.neighbor_in_order(PaneId(8888), 1), None);
}

#[test]
fn neighbor_in_order_single_pane_returns_self() {
    let p = PaneId(1);
    let tree = SplitNode::Leaf(p);
    assert_eq!(tree.neighbor_in_order(p, 1), Some(p));
    assert_eq!(tree.neighbor_in_order(p, -1), Some(p));
}

/// 复用 remove() 验证"移走 pane 后父 Split 折叠"行为符合 detach 语义。
#[test]
fn remove_collapses_parent_split() {
    let (mut tree, ids) = build_nested();
    // 移走 ids[0] → inner_v1 折叠为 Leaf(ids[1]),根的 first 变成 Leaf(ids[1])
    let remaining = tree.remove(ids[0]);
    assert!(remaining.is_some());
    // ids[0] 不再存在,ids[1..5] 仍在
    let ids_after = tree.pane_ids();
    assert_eq!(ids_after.len(), 4);
    assert!(!ids_after.contains(&ids[0]));
    for i in 1..5 { assert!(ids_after.contains(&ids[i])); }
}

#[test]
fn remove_on_root_leaf_returns_none() {
    let p = PaneId(1);
    let mut tree = SplitNode::Leaf(p);
    assert_eq!(tree.remove(p), None);
}

/// GAP-5.6: remap 重写叶子 id,同时保持树形与比例不变。
#[test]
fn remap_pane_ids_rewrites_leaves() {
    let (mut tree, ids) = build_nested();
    let before = tree.pane_ids();
    let mut mapping = std::collections::HashMap::new();
    // 每个 id 都 +1000。
    for id in &before {
        mapping.insert(*id, PaneId(id.0 + 1000));
    }
    tree.remap_pane_ids(&mapping);
    let after = tree.pane_ids();
    assert_eq!(after.len(), before.len());
    for (b, a) in before.iter().zip(after.iter()) {
        assert_eq!(a.0, b.0 + 1000);
    }
    // 原始 id 必须消失。
    for id in &ids {
        assert!(!after.contains(id));
    }
}

/// GAP-5.6: regenerate_split_ids 不得干扰窗格布局。
#[test]
fn regenerate_split_ids_preserves_panes() {
    let (mut tree, _ids) = build_nested();
    let before = tree.pane_ids();
    tree.regenerate_split_ids();
    let after = tree.pane_ids();
    assert_eq!(before, after);
}

/// GAP-5.6:序列化 → 反序列化必须重建相同的叶子顺序
/// 并保留具有代表性的比例(这是跨重启恢复的
/// 持久化路径)。
#[test]
fn serde_round_trip_preserves_structure() {
    let (tree, _ids) = build_nested();
    let json = serde_json::to_string(&tree).expect("serialize");
    let back: SplitNode = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.pane_ids(), tree.pane_ids());
    // 根比例(0.2)得以保留。
    match &back {
        SplitNode::Split { ratio, .. } => assert!((ratio - 0.2).abs() < 1e-6),
        _ => panic!("root must be a Split"),
    }
}

// ── GAP-6.6: resize_ratio / find_adjacent(resize-pane / select-pane 几何) ──
//
// 这两个操作支撑 `tmux resize-pane` 与(桩实现的)几何
// `select-pane -L/-R/-U/-D`。此前 `resize_ratio` 未测试,
// `find_adjacent` 仅通过 layout 间接覆盖;两者都是
// 手动像素冒烟 resize/focus 步骤的关键支撑。

/// `resize_ratio` 把最浅祖先 Split 的比例钳制到
/// [MIN_RATIO, MAX_RATIO] — 巨大增量也不得 panic 或溢出。
#[test]
fn resize_ratio_clamps_to_min_and_max() {
    let (mut tree, ids) = build_nested();
    // ids[0] 位于 root.first → resize_ratio 移动 ROOT 的比例
    // (包含该窗格的最浅祖先 Split)。根为 0.2。
    let mut ratios = Vec::new();

    tree.resize_ratio(ids[0], 100.0);
    collect_ratios(&tree, &mut ratios);
    assert!((ratios[0] - MAX_RATIO).abs() < 1e-6, "delta > MAX clamps: {}", ratios[0]);

    ratios.clear();
    tree.resize_ratio(ids[0], -100.0);
    collect_ratios(&tree, &mut ratios);
    assert!((ratios[0] - MIN_RATIO).abs() < 1e-6, "delta < MIN clamps: {}", ratios[0]);
}

/// 小增量只影响最浅的 Split;路径上更深的 split
/// 保持不变(防止将来出现“把一切 resize 掉”的 bug)。
#[test]
fn resize_ratio_small_delta_only_touches_shallowest_split() {
    let (mut tree, ids) = build_nested();
    // 比例:[root=0.2, inner_v1=0.3, inner_h=0.7, inner_v2=0.4]
    tree.resize_ratio(ids[0], 0.1); // 根 0.2 → 0.3
    let mut ratios = Vec::new();
    collect_ratios(&tree, &mut ratios);
    assert!((ratios[0] - 0.3).abs() < 1e-6, "root ratio moved: {}", ratios[0]);
    assert!((ratios[1] - 0.3).abs() < 1e-6, "deeper inner_v1 unchanged: {}", ratios[1]);
    assert!((ratios[2] - 0.7).abs() < 1e-6, "unrelated inner_h unchanged: {}", ratios[2]);
}

/// `find_adjacent` 返回匹配分割方向上的几何邻居
/// (即 `select-pane -L/-R/-U/-D` 背后的操作)。
#[test]
fn find_adjacent_returns_neighbor_in_matching_direction() {
    // 树形 build_nested: H[ V[a,b], H[c, V[d,e]] ]
    let (tree, ids) = build_nested();
    // a 的水平(右)邻居:跨根 H 分割线 → c (root.second.first_pane)
    assert_eq!(tree.find_adjacent(ids[0], SplitDirection::Horizontal), Some(ids[2]));
    // a 的垂直(下)邻居:同 inner_v1 → b
    assert_eq!(tree.find_adjacent(ids[0], SplitDirection::Vertical), Some(ids[1]));
    // c 的水平(左)邻居 → b (root.first.last_pane)
    assert_eq!(tree.find_adjacent(ids[2], SplitDirection::Horizontal), Some(ids[1]));
}

/// 当请求方向上不存在 Split 时,`find_adjacent`
/// 返回 None(该窗格位于布局的边缘)。
#[test]
fn find_adjacent_none_when_no_split_in_that_direction() {
    // 纯水平树:无垂直分割,任何 pane 的 Vertical 邻居为 None
    let tree = SplitNode::Split {
        split_id: 1,
        direction: SplitDirection::Horizontal,
        ratio: 0.5,
        first: Box::new(SplitNode::Leaf(PaneId(1))),
        second: Box::new(SplitNode::Leaf(PaneId(2))),
    };
    assert_eq!(tree.find_adjacent(PaneId(1), SplitDirection::Vertical), None);
    assert_eq!(tree.find_adjacent(PaneId(1), SplitDirection::Horizontal), Some(PaneId(2)));
}

/// join-pane 树操作序列:从树 A 分离叶子(折叠其父
/// Split)并作为兄弟插入树 B。两棵树都必须保持有效;
/// 有效;A 至少保留 1 个窗格。这是 `apply_attach_pane` 的
/// 树一半(`Arc` 跟随的一半在 `join_pane_e2e` 中)。
#[test]
fn remove_then_split_into_other_tree_yields_valid_shape() {
    let a = PaneId(10);
    let b = PaneId(11);
    let c = PaneId(20);
    let mut tree_a = SplitNode::Split {
        split_id: 1,
        direction: SplitDirection::Vertical,
        ratio: 0.5,
        first: Box::new(SplitNode::Leaf(a)),
        second: Box::new(SplitNode::Leaf(b)),
    };
    let mut tree_b = SplitNode::single(c);

    // 源侧:detach a → A 折叠为单叶 b,仍 ≥1 pane
    assert_eq!(tree_a.remove(a), Some(b));
    assert_eq!(tree_a.pane_count(), 1);
    assert!(matches!(tree_a, SplitNode::Leaf(PaneId(11))));
    assert!(!tree_a.contains(a));

    // 目标侧:把 a 作为 c 的 sibling 插入(split 把 new_pane 放 second)
    assert!(tree_b.split(c, a, SplitDirection::Horizontal));
    assert_eq!(tree_b.pane_count(), 2);
    assert!(tree_b.contains(a) && tree_b.contains(c));
    assert_eq!(tree_b.pane_ids(), vec![c, a]);
}

// ── GAP-2.1: Surface(原 PaneContent)重命名 ──────────────────────────

/// `Surface` 是 PTY/WebView 后端的一等类型名(GAP-2.1 把 `PaneContent`
/// 重命名为 `Surface`)。旧名 `PaneContent` 保留为类型别名,旧引用无需改动。
/// 构造一个 Web surface,经 match 穷尽判别,并确认 `PaneContent` 别名指向
/// 同一类型;`Pane` 访问器沿用既有行为。
#[test]
fn surface_is_canonical_name_with_pane_content_alias() {
    fn kind_of(s: &Surface) -> &'static str {
        match s {
            Surface::Terminal(_) => "terminal",
            Surface::Web(_) => "web",
        }
    }
    let web = Surface::Web(crate::browser::BrowserContext::new("https://example.com"));
    assert_eq!(kind_of(&web), "web");

    // 别名:PaneContent 就是 Surface(同一类型,无转换)。
    let _alias_check: &PaneContent = &web;

    let pane = Pane { id: PaneId(7), content: web, title: "t".into() };
    assert!(pane.is_web());
    assert!(pane.web().is_some());
    assert!(pane.terminal().is_none());
}
