use dioxus::prelude::*;

use crate::models::Group;

/// 「全部」筛选值。
pub const GROUP_ALL: &str = "";
/// 「未分组」筛选值。未分组不占数据库一行，用这个保留值表示。
pub const GROUP_UNGROUPED: &str = "ungrouped";

/// 分组的筛选值：`g:<id>`。
pub fn group_value(id: i64) -> String {
    format!("g:{id}")
}

/// 解析筛选值。无法识别的一律当「全部」，避免留下一个空白的死状态。
pub fn parse_filter(value: &str) -> Filter {
    if value == GROUP_UNGROUPED {
        Filter::Ungrouped
    } else if let Some(id) = value.strip_prefix("g:").and_then(|id| id.parse().ok()) {
        Filter::Group(id)
    } else {
        Filter::All
    }
}

/// 当前选中的分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Ungrouped,
    Group(i64),
}

/// 一个分类标签页。
#[derive(Debug, Clone, PartialEq)]
pub struct GroupTab {
    pub label: String,
    /// 见 [`GROUP_ALL`] / [`GROUP_UNGROUPED`] / [`group_value`]
    pub value: String,
    pub count: usize,
    /// 分组图标地址，空则用首字
    pub icon: String,
}

/// 分类标签页。原来的实现由标签去重派生，现在由显式分组驱动。
#[component]
pub fn GroupTabs(tabs: Vec<GroupTab>, active: String, on_select: EventHandler<String>) -> Element {
    rsx! {
        nav { class: "tabs",
            for tab in tabs {
                button {
                    key: "{tab.value}",
                    class: if tab.value == active { "tab active" } else { "tab" },
                    r#type: "button",
                    title: "{tab.label}",
                    onclick: {
                        let value = tab.value.clone();
                        move |_| on_select.call(value.clone())
                    },
                    if !tab.icon.is_empty() {
                        {super::icons::group_icon(&tab.icon, &tab.label, "group-icon-xs")}
                    }
                    span { class: "tab-name", "{tab.label}" }
                    span { class: "tab-count", "{tab.count}" }
                }
            }
        }
    }
}

/// 按分组的自定义顺序，把「全部 / 未分组 / 各分组」拼成标签页。
///
/// 没有书签的分组不出现（和原来"空标签自动消失"的行为一致），
/// 但只要库里还有未分组的书签，「未分组」这一项就始终在。
///
/// `total` 是结果集里**书签的条数**，不是 `counts` 的和：多分组书签会在多个
/// 分组下各计一次，加起来会大于「全部」。
pub fn build_tabs(
    groups: &[Group],
    counts: &std::collections::HashMap<String, usize>,
    total: usize,
) -> Vec<GroupTab> {
    let mut tabs = vec![GroupTab {
        label: "全部".to_string(),
        value: GROUP_ALL.to_string(),
        count: total,
        icon: String::new(),
    }];

    let ungrouped = counts.get(GROUP_UNGROUPED).copied().unwrap_or(0);
    if ungrouped > 0 {
        tabs.push(GroupTab {
            label: "未分组".to_string(),
            value: GROUP_UNGROUPED.to_string(),
            count: ungrouped,
            icon: String::new(),
        });
    }

    for group in groups {
        let value = group_value(group.id);
        let count = counts.get(&value).copied().unwrap_or(0);
        if count == 0 {
            continue;
        }
        tabs.push(GroupTab {
            label: group.name.clone(),
            value,
            count,
            icon: group.icon.clone(),
        });
    }

    tabs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn group(id: i64, name: &str) -> Group {
        Group {
            id,
            name: name.to_string(),
            icon: String::new(),
        }
    }

    #[test]
    fn filter_values_round_trip() {
        assert_eq!(parse_filter(GROUP_ALL), Filter::All);
        assert_eq!(parse_filter(GROUP_UNGROUPED), Filter::Ungrouped);
        assert_eq!(parse_filter(&group_value(7)), Filter::Group(7));
        // 认不出来的一律当"全部"，不留死状态
        assert_eq!(parse_filter("g:abc"), Filter::All);
        assert_eq!(parse_filter("乱写"), Filter::All);
    }

    #[test]
    fn tabs_skip_empty_groups_but_keep_ungrouped() {
        let groups = vec![group(1, "开发"), group(2, "空分组")];
        let mut counts: HashMap<String, usize> = HashMap::new();
        counts.insert(GROUP_UNGROUPED.to_string(), 2);
        counts.insert(group_value(1), 3);

        // 多分组时 counts 之和会大于书签数（同一条在两个组里各计一次）；
        // 「全部」必须用调用方传进来的书签数，而不是把 counts 加起来。
        let total = 4;
        let tabs = build_tabs(&groups, &counts, total);
        let labels: Vec<&str> = tabs.iter().map(|t| t.label.as_str()).collect();

        assert_eq!(labels, vec!["全部", "未分组", "开发"]);
        assert_eq!(tabs[0].count, 4, "全部用书签数，不重复计数");
        assert_eq!(counts.values().sum::<usize>(), 5, "分组计数允许大于总数");
        assert_eq!(tabs[2].count, 3);
        assert!(!labels.contains(&"空分组"), "没有书签的分组不出现");
    }

    #[test]
    fn ungrouped_tab_disappears_when_empty() {
        let groups = vec![group(1, "开发")];
        let mut counts: HashMap<String, usize> = HashMap::new();
        counts.insert(group_value(1), 1);

        let total: usize = counts.values().sum();
        let tabs = build_tabs(&groups, &counts, total);
        let labels: Vec<&str> = tabs.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(labels, vec!["全部", "开发"]);
    }

    #[test]
    fn tab_order_follows_group_order() {
        let groups = vec![group(9, "乙"), group(3, "甲")];
        let mut counts: HashMap<String, usize> = HashMap::new();
        counts.insert(group_value(9), 1);
        counts.insert(group_value(3), 1);

        let total: usize = counts.values().sum();
        let tabs = build_tabs(&groups, &counts, total);
        let labels: Vec<&str> = tabs.iter().map(|t| t.label.as_str()).collect();
        // 传进来的顺序就是显示顺序，不按 id 重排
        assert_eq!(labels, vec!["全部", "乙", "甲"]);
    }
}
