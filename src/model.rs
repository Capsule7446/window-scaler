use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowId {
    pub hwnd: usize,
    pub pid: u32,
    pub cookie: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Debug)]
pub struct WindowInfo {
    pub id: WindowId,
    pub title: String,
    pub program: String,
    pub size: Option<Size>,
    pub unavailable: Option<String>,
}

impl WindowInfo {
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        query.is_empty()
            || self.title.to_lowercase().contains(&query)
            || self.program.to_lowercase().contains(&query)
            || self.id.pid.to_string().contains(&query)
    }
}

pub fn valid_width(text: &str) -> Option<u32> {
    let text = text.trim();
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<u32>()
        .ok()
        .filter(|n| *n > 0 && *n <= i32::MAX as u32)
}

pub fn scaled_size(width: u32, baseline: Size) -> Result<Size, String> {
    if width == 0 || width > i32::MAX as u32 || baseline.width <= 0 || baseline.height <= 0 {
        return Err("没有有效的窗口尺寸或宽度".into());
    }
    let height = (u64::from(width) * baseline.height as u64 + baseline.width as u64 / 2)
        / baseline.width as u64;
    if height > i32::MAX as u64 {
        return Err("计算后的高度超出系统支持范围".into());
    }
    Ok(Size {
        width: width as i32,
        height: height.max(1) as i32,
    })
}

pub fn hidden_selected(windows: &[WindowInfo], selected: &HashSet<WindowId>, query: &str) -> usize {
    windows
        .iter()
        .filter(|w| selected.contains(&w.id) && !w.matches(query))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_rejects_invalid_and_overflow_without_losing_last_setting() {
        for text in ["", "0", "-1", "2.5", "+12", "4294967296", "2147483648"] {
            assert_eq!(valid_width(text), None, "{text}");
        }
        assert_eq!(valid_width(" 240 "), Some(240));
    }

    #[test]
    fn all_scales_use_original_ratio_instead_of_rounded_previous_result() {
        let original = Size {
            width: 1600,
            height: 900,
        };
        assert_eq!(scaled_size(241, original).unwrap().height, 136);
        assert_eq!(scaled_size(480, original).unwrap().height, 270);
        assert_eq!(scaled_size(240, original).unwrap().height, 135);
        assert!(scaled_size(u32::MAX, original).is_err());
        assert!(
            scaled_size(
                100,
                Size {
                    width: 1,
                    height: i32::MAX
                }
            )
            .is_err()
        );
    }

    #[test]
    fn filtering_preserves_hidden_selection_and_counts_it() {
        let a = WindowInfo {
            id: WindowId {
                hwnd: 1,
                pid: 12,
                cookie: 1,
            },
            title: "星儿".into(),
            program: "LaTale.exe".into(),
            size: None,
            unavailable: None,
        };
        let b = WindowInfo {
            id: WindowId {
                hwnd: 2,
                pid: 34,
                cookie: 2,
            },
            title: "浏览器".into(),
            program: "Chrome.exe".into(),
            size: None,
            unavailable: None,
        };
        let selection = HashSet::from([a.id, b.id]);
        let windows = vec![a, b];
        assert_eq!(hidden_selected(&windows, &selection, "星儿"), 1);
        assert_eq!(selection.len(), 2);
        assert!(windows[1].matches("CHROME"));
        assert!(windows[1].matches("34"));
    }
}
