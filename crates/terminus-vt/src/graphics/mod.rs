//! The graphics store: kitty graphics protocol images and placements, sixel images and iTerm2
//! inline images, kept outside the grid.
//!
//! Row positions are serials: a `u64` per grid row that never changes while the row exists. A placement
//! anchors at (serial, column) and covers `rows` rows with consecutive serials starting at its anchor.
//! This module depends on nothing else in the crate, has no `unsafe`, and builds for wasm (file transmission
//! is compiled out there and answers with an error). Shared memory is read by the host's
//! `SharedMemoryReader`, so the crate itself never opens an operating system object.

mod anim;
mod command;
mod delete;
mod iterm;
mod kitty;
mod media;
mod pixels;
mod place;
mod reply;
mod sixel;

use std::sync::Arc;

pub use command::{KittyCommand, KittyError, parse_kitty_command};

/// Default quota in bytes of decoded pixels (320 MiB).
pub const DEFAULT_QUOTA: usize = 320 * 1024 * 1024;

/// Default most pixels one image may have (10000 by 10000).
pub const DEFAULT_MAX_PIXELS: u64 = 100_000_000;

/// What the terminal tells the store about where a command applies.
#[derive(Clone, Copy, Debug, Default)]
pub struct GraphicsContext {
    /// Serial of the cursor's row (see `PageList::serial_of`).
    pub cursor_serial: u64,
    /// Cursor column.
    pub cursor_col: u16,
    /// Serial of active row 0 (the top of the screen).
    pub screen_top_serial: u64,
    /// Serial of the oldest row still kept; placements above it are gone.
    pub first_serial: u64,
    /// Columns.
    pub cols: u16,
    /// Rows.
    pub rows: u16,
    /// Size of one cell in pixels.
    pub cell_px: (u16, u16),
    /// Whether file, temporary file and shared memory transmission are allowed.
    pub allow_file_transfer: bool,
    /// How to read a shared memory object, from the host; None answers `t=s` with EINVAL.
    pub shared_memory: Option<SharedMemoryReader>,
}

/// Why a shared memory read failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShmError {
    /// The name is invalid, the object does not exist, or the offset and size lie outside it.
    Unreadable,
    /// The requested size is over the quota.
    TooLarge,
    /// The host cannot read shared memory on this platform.
    Unsupported,
}

/// Reads `size` bytes (the rest of the object when 0) from `offset` of the shared memory object
/// `name`, at most `max_bytes`. The host supplies it (`TerminalConfig::shared_memory`).
pub type SharedMemoryReader = fn(name: &str, offset: u64, size: u64, max_bytes: usize) -> Result<Vec<u8>, ShmError>;

/// What the terminal must do after a graphics command.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphicsResult {
    /// Bytes to send back to the program.
    pub reply: Option<Vec<u8>>,
    /// Move the cursor this many columns right and rows down (rows down may scroll).
    /// For a kitty or iTerm2 image this is (columns, rows) of the placement, as the kitty specification says:
    /// "the cursor must be moved to the right by the number of cols in the image placement rectangle and
    /// down by the number of rows". For sixel see `cursor_to_column_zero`.
    pub move_cursor: Option<(u16, u16)>,
    /// When true the cursor column becomes 0 before `move_cursor` is applied (sixel: the cursor ends at
    /// the start of the line after the image, `move_cursor` is then (0, rows)).
    pub cursor_to_column_zero: bool,
    /// The visible picture changed.
    pub dirty: bool,
}

/// One frame of an image. The first frame is the image's own pixels.
#[derive(Clone, Debug)]
pub struct Frame {
    /// Straight alpha RGBA8, `width * height * 4` bytes, shared with the renderer.
    pub rgba: Arc<Vec<u8>>,
    /// Milliseconds before the next frame: 0 for the root frame until set, negative for a gapless frame.
    pub gap_ms: i32,
}

/// Whether an animation steps by itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AnimationState {
    /// Not running (the default, and `s=1`).
    #[default]
    Stopped,
    /// Running but waiting for more frames at the last one (`s=2`).
    Loading,
    /// Running and looping (`s=3`).
    Running,
}

/// Animation settings and progress for an image.
#[derive(Clone, Copy, Debug, Default)]
pub struct Animation {
    /// Whether the animation steps.
    pub state: AnimationState,
    /// Times to play before stopping: 0 is forever (`v=1`), n is `v=n+1`.
    pub loops: u32,
    /// Loops completed since the animation started.
    pub loops_done: u32,
    /// Time at which the next frame change is due, once the animation has been ticked.
    pub(crate) next_change_ms: Option<u64>,
}

/// A stored image.
#[derive(Clone, Debug)]
pub struct Image {
    /// Image id (`i`). For an image sent without `i` or `I`, `sixel` or iTerm2 this is an id the store
    /// chose, and `anonymous` is true.
    pub id: u32,
    /// Image number (`I`), 0 if none.
    pub number: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixels of the first frame, straight alpha RGBA8.
    pub rgba: Arc<Vec<u8>>,
    /// Every frame, the first one being `rgba`. Never empty.
    pub frames: Vec<Frame>,
    /// Index into `frames` of the frame to draw.
    pub current_frame: usize,
    /// Animation settings.
    pub animation: Animation,
    /// The client did not name this image, so replies and placement ids ignore it.
    pub anonymous: bool,
    /// The client marked the image transient (`N=1`).
    pub transient: bool,
    /// Transmitted with `a=t`: it stays when its placements go away.
    pub(crate) persistent: bool,
}

impl Image {
    /// The pixels of the frame to draw.
    pub fn current_rgba(&self) -> &Arc<Vec<u8>> {
        &self.frames[self.current_frame.min(self.frames.len() - 1)].rgba
    }

    /// Bytes of pixel data this image holds across all frames.
    pub fn bytes(&self) -> usize {
        self.frames.iter().map(|f| f.rgba.len()).sum()
    }
}

/// A place where an image is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    /// The image drawn.
    pub image_id: u32,
    /// Placement id (`p`), 0 if none.
    pub placement_id: u32,
    /// Serial of the row of the top left cell (for a relative placement, already resolved from its parent).
    pub serial: u64,
    /// Column of the top left cell.
    pub col: u16,
    /// Z index: negative draws under the text.
    pub z: i32,
    /// Pixel offset inside the first cell, horizontally (`X`).
    pub x_offset: u32,
    /// Pixel offset inside the first cell, vertically (`Y`).
    pub y_offset: u32,
    /// Source rectangle in image pixels (x, y, width, height), clipped to the image and never empty.
    pub src: (u32, u32, u32, u32),
    /// Columns covered.
    pub cols: u16,
    /// Rows covered.
    pub rows: u16,
    /// A virtual placement (`U=1`) is a prototype for Unicode placeholders and is never drawn itself.
    pub virtual_placement: bool,
    /// The (image id, placement id) of the parent for a relative placement.
    pub parent: Option<(u32, u32)>,
    /// Offset in cells (horizontal, vertical) from the parent's top left cell.
    pub offset_from_parent: (i32, i32),
    /// Stretch the image to fill the cells, ignoring its aspect ratio (iTerm2 `preserveAspectRatio=0`).
    /// Otherwise the image is scaled to fit inside the cells with its aspect ratio kept.
    pub stretch: bool,
    /// Creation order, for stable sorting.
    pub uid: u64,
}

/// The graphics store.
#[derive(Debug)]
pub struct GraphicsStore {
    quota: usize,
    max_pixels: u64,
    images: Vec<Image>,
    placements: Vec<Placement>,
    pending: Option<kitty::Pending>,
    used: usize,
    next_uid: u64,
    next_number_id: u32,
    next_anonymous_id: u32,
    generation: u64,
    dropped: u64,
}

impl Default for GraphicsStore {
    /// An empty store with the default quota.
    fn default() -> GraphicsStore {
        GraphicsStore::new(DEFAULT_QUOTA)
    }
}

impl GraphicsStore {
    /// An empty store with a quota in bytes of decoded pixels.
    pub fn new(quota: usize) -> GraphicsStore {
        GraphicsStore {
            quota,
            max_pixels: DEFAULT_MAX_PIXELS,
            images: Vec::new(),
            placements: Vec::new(),
            pending: None,
            used: 0,
            next_uid: 1,
            next_number_id: 1,
            next_anonymous_id: u32::MAX,
            generation: 0,
            dropped: 0,
        }
    }

    /// Sets the most pixels one image may have; larger images are answered with EFBIG.
    pub fn set_max_pixels(&mut self, max_pixels: u64) {
        self.max_pixels = max_pixels;
    }

    /// Removes every image and placement, and abandons a chunked upload in progress.
    pub fn clear(&mut self) {
        let changed = !self.images.is_empty() || !self.placements.is_empty();
        self.images.clear();
        self.placements.clear();
        self.pending = None;
        self.used = 0;
        if changed {
            self.touch();
        }
    }

    /// Bytes of decoded image data held.
    pub fn used_bytes(&self) -> usize {
        self.used
    }

    /// Every image, oldest first.
    pub fn images(&self) -> impl Iterator<Item = &Image> {
        self.images.iter()
    }

    /// The image with this id.
    pub fn image(&self, id: u32) -> Option<&Image> {
        self.images.iter().find(|i| i.id == id)
    }

    /// Every placement, virtual ones included, in creation order.
    pub fn placements(&self) -> impl Iterator<Item = &Placement> {
        self.placements.iter()
    }

    /// The real (not virtual) placements that cover at least one row with serial in the inclusive range.
    pub fn placements_in_rows(&self, first_serial: u64, last_serial: u64) -> Vec<&Placement> {
        let covers = |p: &&Placement| {
            let last = p.serial + (p.rows as u64).max(1) - 1;
            !p.virtual_placement && p.serial <= last_serial && last >= first_serial
        };
        self.placements.iter().filter(covers).collect()
    }

    /// The real placements in drawing order: by z index, then image id (a lower id draws first), then creation.
    pub fn placements_by_z(&self) -> Vec<&Placement> {
        let mut list: Vec<&Placement> = self.placements.iter().filter(|p| !p.virtual_placement).collect();
        list.sort_by_key(|p| (p.z, p.image_id, p.uid));
        list
    }

    /// The virtual placement a Unicode placeholder refers to. With placement id 0 any virtual placement of the
    /// image matches (the oldest).
    pub fn virtual_placement(&self, image_id: u32, placement_id: u32) -> Option<&Placement> {
        let hit = |p: &&Placement| {
            p.virtual_placement && p.image_id == image_id && (placement_id == 0 || p.placement_id == placement_id)
        };
        self.placements.iter().find(hit)
    }

    /// A counter that increases on every change that can alter what is drawn.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Payloads dropped because they were too large or malformed.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Drops placements anchored in rows that were pruned or cleared (serial below `first_serial`), then frees
    /// images that were only transmitted for display (`a=T`, sixel, iTerm2) and are no longer referenced.
    /// Images transmitted with `a=t` stay, as the protocol keeps data that was sent for later placement.
    pub fn prune_before(&mut self, first_serial: u64) {
        let gone = |p: &Placement| !p.virtual_placement && p.serial < first_serial;
        let touched = self.remove_placements(&gone);
        if touched.is_empty() {
            return;
        }
        for id in touched {
            let unreferenced = !self.placements.iter().any(|p| p.image_id == id);
            let transient_data = self.image(id).is_some_and(|i| !i.persistent);
            if unreferenced && transient_data {
                self.remove_image(id);
            }
        }
    }

    /// Marks the picture changed.
    pub(crate) fn touch(&mut self) {
        self.generation += 1;
    }

    /// Allocates the next creation order number.
    pub(crate) fn next_uid(&mut self) -> u64 {
        let uid = self.next_uid;
        self.next_uid += 1;
        uid
    }

    /// Counts a payload that was dropped.
    pub(crate) fn count_dropped(&mut self) {
        self.dropped += 1;
    }

    /// Limits for decoding, derived from the quota and the pixel cap.
    pub(crate) fn decode_limits(&self) -> pixels::DecodeLimits {
        pixels::DecodeLimits { max_pixels: self.max_pixels, quota: self.quota }
    }
}
