use crate::model::Size;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug)]
pub struct TilingOptions {
    pub corner: Corner,
    pub direction: Direction,
    pub gap: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn size(self) -> Result<Size, String> {
        let width = self.right.checked_sub(self.left).filter(|n| *n > 0);
        let height = self.bottom.checked_sub(self.top).filter(|n| *n > 0);
        match (width, height) {
            (Some(width), Some(height)) => Ok(Size { width, height }),
            _ => Err("窗口或工作区没有有效尺寸".into()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

pub fn valid_gap(text: &str) -> Option<u32> {
    let text = text.trim();
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<u32>().ok().filter(|n| *n <= i32::MAX as u32)
}

/// Shelf layout in list order. Coordinates stay in physical screen pixels.
pub fn plan(area: Rect, sizes: &[Size], options: TilingOptions) -> Result<Vec<Position>, String> {
    let available = area.size()?;
    if options.gap > i32::MAX as u32 {
        return Err("排列间隔必须是有效的非负整数".into());
    }
    let horizontal = options.direction == Direction::Horizontal;
    let (main_limit, cross_limit) = if horizontal {
        (available.width, available.height)
    } else {
        (available.height, available.width)
    };
    let (mut main, mut cross, mut lane) = (0i64, 0i64, 0i64);
    let mut positions = Vec::with_capacity(sizes.len());
    for size in sizes {
        if size.width <= 0 || size.height <= 0 {
            return Err("窗口没有有效尺寸".into());
        }
        let (along, across) = if horizontal {
            (i64::from(size.width), i64::from(size.height))
        } else {
            (i64::from(size.height), i64::from(size.width))
        };
        if main > 0 && main + along > i64::from(main_limit) {
            main = 0;
            cross += lane + i64::from(options.gap);
            lane = 0;
        }
        if along > i64::from(main_limit) || cross + across > i64::from(cross_limit) {
            return Err("排列空间不足，未移动窗口；请先手动缩小窗口、减少选择或减小间隔".into());
        }
        let (dx, dy) = if horizontal {
            (main, cross)
        } else {
            (cross, main)
        };
        let x = if matches!(options.corner, Corner::TopRight | Corner::BottomRight) {
            i64::from(area.right) - dx - i64::from(size.width)
        } else {
            i64::from(area.left) + dx
        };
        let y = if matches!(options.corner, Corner::BottomLeft | Corner::BottomRight) {
            i64::from(area.bottom) - dy - i64::from(size.height)
        } else {
            i64::from(area.top) + dy
        };
        positions.push(Position {
            x: x as i32,
            y: y as i32,
        });
        main += along + i64::from(options.gap);
        lane = lane.max(across);
    }
    Ok(positions)
}
