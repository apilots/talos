//! In-memory directory cursor. Keep the iterator across slices, never a path
//! bookmark or an offset into a freshly enumerated directory.
use super::*;

pub(super) struct Walk {
    root: PathBuf,
    frames: Vec<Frame>,
    started: bool,
    prune: bool,
}
struct Frame {
    path: PathBuf,
    entries: fs::ReadDir,
    depth: usize,
}
pub(super) enum Item {
    Visited,
    Pruned,
    Log(PathBuf),
    Complete,
}
impl Walk {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            frames: Vec::new(),
            started: false,
            prune: false,
        }
    }
    pub fn pruning(root: PathBuf) -> Self {
        Self {
            prune: true,
            ..Self::new(root)
        }
    }
    pub fn next(&mut self) -> TalosResult<Item> {
        if !self.started {
            self.started = true;
            self.frames.push(Frame {
                entries: fs::read_dir(&self.root).map_err(io_error)?,
                path: self.root.clone(),
                depth: 3,
            });
        }
        loop {
            let Some(frame) = self.frames.last_mut() else {
                return Ok(Item::Complete);
            };
            match frame.entries.next() {
                Some(entry) => {
                    let entry = entry.map_err(io_error)?;
                    let kind = entry.file_type().map_err(io_error)?;
                    if !kind.is_dir() || kind.is_symlink() {
                        return Ok(Item::Visited);
                    }
                    if frame.depth == 1 {
                        return Ok(Item::Log(entry.path()));
                    }
                    let next = Frame {
                        entries: fs::read_dir(entry.path()).map_err(io_error)?,
                        path: entry.path(),
                        depth: frame.depth - 1,
                    };
                    self.frames.push(next);
                    return Ok(Item::Visited);
                }
                None => {
                    let frame = self.frames.pop().expect("directory frame exists");
                    if self.prune
                        && frame.path != self.root
                        && maintenance::remove_if_empty(&frame.path)?
                    {
                        return Ok(Item::Pruned);
                    }
                }
            }
        }
    }
}
