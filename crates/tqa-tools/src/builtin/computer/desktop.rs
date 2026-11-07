//! The desktop driver (ADR-0039 slice 2): one screen, routed by the focused window. A window of the runtime's own browser
//! goes to the browser driver (its page's DOM refs come first); any other application's goes to its AT-SPI tree, with
//! XTEST input at an element's centre as the fallback for an element without the action asked for. Keys and text
//! without a ref are XTEST whatever has the focus, as a person's keyboard. Handles carry their route: `b:` for the
//! browser's, `a:` for the tree's.
use std::{
    collections::hash_map::DefaultHasher,
    future::Future,
    hash::{Hash, Hasher},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;

use super::{
    a11y::{A11y, elements},
    driver::{
        DriverError, ElementAct, FileInput, PointerAct, RawSnapshot, ScreenDriver, ScreenState,
        Source,
    },
    framebuffer::Framebuffer,
    raster::{Raster, Rect},
    x11::{ActiveWindow, XDo},
};

/// The accessibility actions a click may perform, in AT-SPI's names, preferred in order.
const CLICK_ACTIONS: [&str; 5] = ["click", "press", "activate", "jump", "toggle"];
/// How long one call to an application's accessibility tree may take. An application can stop answering it (a modal
/// dialog of Thunar's or Ark's in the agent eval, 6 Nov 2026): the screen is never held for it (TOOL-U-138).
const TREE_BOUND: Duration = Duration::from_secs(3);
/// How long reading a focused window's whole tree may take: one call per node, so a large window (an office
/// document's) needs more than one call's bound, and a stalled one still costs seconds, not minutes.
const WINDOW_BOUND: Duration = Duration::from_secs(10);
/// The window a snapshot or state names while no window has the focus.
const NO_WINDOW: &str = "none";
/// The title a snapshot shows while no window has the focus.
const NO_WINDOW_TITLE: &str = "no window has the focus";
/// How Chromium titles its page's window: "<page title> - Chromium". A page sets its own title but can't make a
/// second window, so another window of the browser's process (GTK's Open File, Print) never ends so.
const PAGE_WINDOW_SUFFIX: &str = " - Chromium";

/// The runtime's browser: its driver and its process.
pub struct OwnBrowser {
    /// The browser driver.
    pub driver: Arc<dyn ScreenDriver>,
    /// Its process, as its windows' `_NET_WM_PID` names it.
    pub pid: u32,
}

/// The whole screen.
pub struct Desktop {
    browser: Option<OwnBrowser>,
    x: XDo,
    tree: Arc<dyn A11y>,
    screen: Option<Framebuffer>,
}

/// Where the focused window's input goes.
enum Focus {
    Browser(ActiveWindow),
    Application(ActiveWindow, u32),
    Unknown(ActiveWindow),
    /// No window has the focus (an empty desktop): the screen is still seen whole, there are no elements, and keys go
    /// to the display.
    Nothing,
}

impl Desktop {
    /// The screen: the runtime's browser (if it has started one), the display's input, the accessibility tree and the
    /// screen file for the settle rule's digest (`None` falls back to window titles).
    #[must_use]
    pub fn new(
        browser: Option<OwnBrowser>,
        x: XDo,
        tree: Arc<dyn A11y>,
        screen: Option<Framebuffer>,
    ) -> Self {
        Self {
            browser,
            x,
            tree,
            screen,
        }
    }

    async fn focus(&self) -> Result<Focus, DriverError> {
        let Some(active) = self.x.active().await? else {
            return Ok(Focus::Nothing);
        };
        Ok(match (active.pid, &self.browser) {
            // Only the page's own window is the page: any other window of the browser is read as an application's,
            // so the page's refs and marks never stand over it (TOOL-U-063, c16k real-world A12b).
            (Some(pid), Some(browser))
                if pid == browser.pid && active.title.ends_with(PAGE_WINDOW_SUFFIX) =>
            {
                Focus::Browser(active)
            }
            (Some(pid), _) => Focus::Application(active, pid),
            (None, _) => Focus::Unknown(active),
        })
    }

    fn browser(&self) -> Result<&Arc<dyn ScreenDriver>, DriverError> {
        self.browser
            .as_ref()
            .map(|browser| &browser.driver)
            .ok_or_else(|| DriverError::Unavailable("the browser isn't running".into()))
    }

    /// A digest of the whole screen, else of what's known (the title and the window).
    async fn digest(&self, fallback: impl Hash) -> u64 {
        if let Some(screen) = &self.screen
            && let Some(digest) = screen.digest().await
        {
            return digest;
        }
        let mut hasher = DefaultHasher::new();
        fallback.hash(&mut hasher);
        hasher.finish()
    }

    /// An element's own action, else a click at its centre.
    async fn click(&self, handle: &str, count: u8) -> Result<(), DriverError> {
        if count == 1 && bounded(self.tree.act(handle, &CLICK_ACTIONS)).await? {
            return Ok(());
        }
        let centre = self.centre(handle).await?;
        self.x.click(centre, count).await
    }

    async fn centre(&self, handle: &str) -> Result<(i32, i32), DriverError> {
        let (x, y, width, height) = bounded(self.tree.extents(handle))
            .await?
            .ok_or_else(|| DriverError::Failed("the element has no place on the screen".into()))?;
        Ok((x + width / 2, y + height / 2))
    }

    /// Its text set, else the focus given and the text typed.
    async fn type_into(&self, handle: &str, text: &str) -> Result<(), DriverError> {
        // A value holding a line break or a tab would hold the character, not submit or move on (TOOL-U-140).
        let keys = text.contains(['\n', '\t']);
        if !keys && bounded(self.tree.set_text(handle, text)).await? {
            return Ok(());
        }
        if !bounded(self.tree.focus(handle)).await? {
            let centre = self.centre(handle).await?;
            self.x.click(centre, 1).await?;
        }
        self.x.type_text(text).await
    }
}

impl Desktop {
    /// The focused window's snapshot; `marked` asks the browser for its elements' boxes too (an application's come
    /// with its tree's walk).
    async fn snapshot_of(&self, marked: bool) -> Result<RawSnapshot, DriverError> {
        match self.focus().await? {
            Focus::Browser(active) => {
                let browser = self.browser()?;
                let mut snapshot = if marked {
                    browser.marked_snapshot().await?
                } else {
                    browser.snapshot().await?
                };
                snapshot.window = format!("{}/{}", active.id, snapshot.window);
                for element in &mut snapshot.elements {
                    element.handle = format!("b:{}", element.handle);
                }
                Ok(snapshot)
            }
            Focus::Application(active, pid) => {
                // A tree that doesn't answer leaves the snapshot without the application's elements; the screenshot
                // still goes to the model, and its marks (if any) still come.
                let window = match within(WINDOW_BOUND, self.tree.window(pid)).await {
                    Ok(window) => window,
                    Err(DriverError::Unavailable(reason)) => {
                        tracing::warn!(%reason, "desktop: the snapshot goes without the application's tree");
                        None
                    }
                    Err(error) => return Err(error),
                };
                let (title, mut found) = window.map_or_else(
                    || (active.title.clone(), Vec::new()),
                    |window| (window.title, elements(&window.nodes)),
                );
                // A box outside the window's frame (a GTK4 tree's, relative to the window) is no place on the screen:
                // the element stays listed for its own actions, without a mark (TOOL-U-139).
                if let Some(frame) = self.x.geometry(&active.id).await {
                    found
                        .iter_mut()
                        .filter(|element| element.rect.is_some_and(|rect| !inside(rect, frame)))
                        .for_each(|element| element.rect = None);
                }
                for element in &mut found {
                    element.handle = format!("a:{}", element.handle);
                }
                Ok(RawSnapshot {
                    source: Source::A11y,
                    window: active.id,
                    title,
                    elements: found,
                })
            }
            Focus::Unknown(active) => Ok(RawSnapshot {
                source: Source::A11y,
                window: active.id,
                title: active.title,
                elements: Vec::new(),
            }),
            Focus::Nothing => Ok(RawSnapshot {
                source: Source::A11y,
                window: NO_WINDOW.into(),
                title: NO_WINDOW_TITLE.into(),
                elements: Vec::new(),
            }),
        }
    }
}

/// Whether a box's centre lies inside a window's frame (x, y, width, height).
fn inside(rect: Rect, (x, y, width, height): (i32, i32, u32, u32)) -> bool {
    let (cx, cy) = rect.centre();
    let right = i64::from(x) + i64::from(width);
    let bottom = i64::from(y) + i64::from(height);
    (i64::from(x)..right).contains(&cx) && (i64::from(y)..bottom).contains(&cy)
}

/// One call to the accessibility tree within [`TREE_BOUND`]; past it, the tree is unavailable.
async fn bounded<T>(call: impl Future<Output = Result<T, DriverError>>) -> Result<T, DriverError> {
    within(TREE_BOUND, call).await
}

/// A call to the accessibility tree within `bound`; past it, the tree is unavailable.
async fn within<T>(
    bound: Duration,
    call: impl Future<Output = Result<T, DriverError>>,
) -> Result<T, DriverError> {
    tokio::time::timeout(bound, call).await.unwrap_or_else(|_| {
        Err(DriverError::Unavailable(format!(
            "the application's accessibility tree didn't answer within {} s",
            bound.as_secs()
        )))
    })
}

#[async_trait]
impl ScreenDriver for Desktop {
    async fn snapshot(&self) -> Result<RawSnapshot, DriverError> {
        self.snapshot_of(false).await
    }

    async fn marked_snapshot(&self) -> Result<RawSnapshot, DriverError> {
        self.snapshot_of(true).await
    }

    async fn pointer(&self, at: (i32, i32), act: PointerAct) -> Result<(), DriverError> {
        match act {
            PointerAct::Click(count) => self.x.click(at, count).await,
            PointerAct::Scroll(direction, notches) => self.x.wheel(at, direction, notches).await,
        }
    }

    async fn drag(&self, from: (i32, i32), to: (i32, i32)) -> Result<(), DriverError> {
        self.x.drag(from, to).await
    }

    async fn screen(&self) -> Result<Raster, DriverError> {
        match &self.screen {
            Some(screen) => screen.raster().await,
            None => Err(DriverError::Unavailable(
                "there is no screen file to read".into(),
            )),
        }
    }

    async fn state(&self) -> Result<ScreenState, DriverError> {
        match self.focus().await? {
            Focus::Browser(active) => {
                let page = self.browser()?.state().await?;
                let digest = self.digest((&page.title, page.digest)).await;
                Ok(ScreenState {
                    window: format!("{}/{}", active.id, page.window),
                    digest,
                    ..page
                })
            }
            Focus::Application(active, _) | Focus::Unknown(active) => {
                let digest = self.digest((&active.id, &active.title)).await;
                Ok(ScreenState {
                    window: active.id,
                    title: active.title,
                    dialog: None,
                    digest,
                })
            }
            Focus::Nothing => Ok(ScreenState {
                window: NO_WINDOW.into(),
                title: NO_WINDOW_TITLE.into(),
                dialog: None,
                digest: self.digest(NO_WINDOW).await,
            }),
        }
    }

    async fn act(&self, handle: &str, act: ElementAct) -> Result<(), DriverError> {
        if let Some(inner) = handle.strip_prefix("b:") {
            return self.browser()?.act(inner, act).await;
        }
        let handle = handle.strip_prefix("a:").ok_or(DriverError::Gone)?;
        match act {
            ElementAct::Click => self.click(handle, 1).await,
            ElementAct::DoubleClick => self.click(handle, 2).await,
            ElementAct::Scroll(direction, notches) => {
                let centre = self.centre(handle).await?;
                self.x.wheel(centre, direction, notches).await
            }
            ElementAct::Type(text) => self.type_into(handle, &text).await,
        }
    }

    // Keys and text go through XTEST whatever has the focus, as a person's keyboard does: the browser's own UI (ctrl+l
    // and its address bar) never sees the page's debugging protocol input (ADR-0039 section 2, amended 12 Oct 2026).
    async fn type_focused(&self, text: &str) -> Result<(), DriverError> {
        self.x.type_text(text).await
    }

    async fn key(&self, keys: &[String]) -> Result<(), DriverError> {
        self.x.key(keys).await
    }

    // File inputs are the runtime's browser's: its handles carry the `b:` route, as its elements' do.
    async fn take_file_chooser(&self) -> Result<Option<FileInput>, DriverError> {
        let Some(browser) = &self.browser else {
            return Ok(None);
        };
        Ok(browser
            .driver
            .take_file_chooser()
            .await?
            .map(|input| routed(input, "b:")))
    }

    async fn file_input(&self, handle: &str) -> Result<Option<FileInput>, DriverError> {
        let Some(inner) = handle.strip_prefix("b:") else {
            return Ok(None);
        };
        Ok(self
            .browser()?
            .file_input(inner)
            .await?
            .map(|input| routed(input, "b:")))
    }

    async fn choose_files(
        &self,
        input: &FileInput,
        files: &[std::path::PathBuf],
    ) -> Result<(), DriverError> {
        let inner = input.handle.strip_prefix("b:").ok_or(DriverError::Gone)?;
        let input = FileInput {
            handle: inner.to_owned(),
            multiple: input.multiple,
        };
        self.browser()?.choose_files(&input, files).await
    }
}

/// A file input with its handle's route in front.
fn routed(mut input: FileInput, route: &str) -> FileInput {
    input.handle.insert_str(0, route);
    input
}
