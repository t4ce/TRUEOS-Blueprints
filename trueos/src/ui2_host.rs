#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct WindowId(u32);

impl WindowId {
    #[inline]
    pub const fn new(raw: u32) -> Option<Self> {
        if raw == 0 { None } else { Some(Self(raw)) }
    }

    #[inline]
    pub const fn raw(self) -> u32 {
        self.0
    }

    pub fn info(self) -> Option<WindowInfo> {
        let _ = self;
        None
    }

    pub fn set_title(self, _title: &str) -> bool {
        let _ = self;
        false
    }

    pub fn set_icon(self, _icon_id: u32) -> bool {
        let _ = self;
        false
    }

    pub fn set_position(self, _x: i32, _y: i32) -> bool {
        let _ = self;
        false
    }

    pub fn set_size(self, _width: u32, _height: u32) -> bool {
        let _ = self;
        false
    }

    pub fn set_decorations(self, _mode: WindowDecorationMode) -> bool {
        let _ = self;
        false
    }

    pub fn set_hit_test_visible(self, _visible: bool) -> bool {
        let _ = self;
        false
    }

    pub fn set_vertical_scrollbar_side(self, _side: VerticalScrollbarSide) -> bool {
        let _ = self;
        false
    }

    pub fn set_horizontal_scrollbar_side(self, _side: HorizontalScrollbarSide) -> bool {
        let _ = self;
        false
    }

    pub fn minimize(self) -> bool {
        let _ = self;
        false
    }

    pub fn maximize(self) -> bool {
        let _ = self;
        false
    }

    pub fn restore(self) -> bool {
        let _ = self;
        false
    }

    pub fn focus(self) -> bool {
        let _ = self;
        false
    }

    pub fn close(self) -> bool {
        let _ = self;
        false
    }

    pub fn begin_move(self) -> bool {
        let _ = self;
        false
    }

    pub fn begin_resize(self, _edge_mask: u32) -> bool {
        let _ = self;
        false
    }
}

#[derive(Debug)]
pub struct OwnedWindow {
    id: WindowId,
    close_on_drop: bool,
}

impl OwnedWindow {
    pub fn create(title: &str, rect: Rect) -> Option<Self> {
        Self::create_with_options(title, rect, CreateOptions::default())
    }

    pub fn create_with_options(_title: &str, _rect: Rect, _options: CreateOptions) -> Option<Self> {
        None
    }

    pub fn from_existing(id: WindowId) -> Self {
        Self {
            id,
            close_on_drop: false,
        }
    }

    #[inline]
    pub const fn id(&self) -> WindowId {
        self.id
    }

    pub fn info(&self) -> Option<WindowInfo> {
        self.id.info()
    }

    pub fn leak(mut self) -> WindowId {
        self.close_on_drop = false;
        self.id
    }
}

impl Drop for OwnedWindow {
    fn drop(&mut self) {
        if self.close_on_drop {
            let _ = self.id.close();
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct CreateOptions {
    pub z: i32,
    pub alpha: u8,
}

impl Default for CreateOptions {
    fn default() -> Self {
        Self { z: 0, alpha: 255 }
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub const RESIZE_LEFT: u32 = 1 << 0;
pub const RESIZE_TOP: u32 = 1 << 1;
pub const RESIZE_RIGHT: u32 = 1 << 2;
pub const RESIZE_BOTTOM: u32 = 1 << 3;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WindowState {
    Normal,
    Minimized,
    Maximized,
    Unknown(u32),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WindowDecorationMode {
    System = 0,
    Client = 1,
    None = 2,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum VerticalScrollbarSide {
    Left = 0,
    Right = 1,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum HorizontalScrollbarSide {
    Top = 0,
    Bottom = 1,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct WindowInfo {
    pub id: WindowId,
    pub kind: u32,
    pub state: WindowState,
    pub decoration_mode: u32,
    pub icon_id: u32,
    pub visible: bool,
    pub hit_test_visible: bool,
    pub selected: bool,
    pub frame: Rect,
    pub content: Rect,
    pub decoration: Rect,
}

pub fn primary_browser_window() -> Option<WindowId> {
    None
}