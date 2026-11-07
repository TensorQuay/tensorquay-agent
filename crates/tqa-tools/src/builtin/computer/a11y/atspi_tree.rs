//! The session's accessibility tree over AT-SPI, read with our own zbus proxies ([`super::atspi_bus`]; ADR-0039 slice
//! 2, amended 18 Oct 2026): the application found by its process, its active window walked within bounds, and an
//! element's actions, text and focus.
use std::{collections::HashMap, sync::Mutex};

use async_trait::async_trait;
use zbus::{
    Connection, ObjectPath,
    names::BusName,
    proxy::{Builder, CacheProperties, Defaults},
};

use super::{
    A11y, Node, Window,
    atspi_bus::{
        AccessibleProxy, ActionProxy, BusProxy, ComponentProxy, EditableTextProxy, SCREEN,
        TextProxy, ValueProxy,
    },
    atspi_wire::{
        Dock, Place, TEXT, VALUE, action_index, dock_shift, dock_step, has, named, states,
        thumbnail_offset,
    },
    listed,
};
use crate::builtin::computer::{driver::DriverError, snapshot::shown_name};

/// The registry's root, whose children are the applications.
const REGISTRY: &str = "org.a11y.atspi.Registry";
const ROOT: &str = "/org/a11y/atspi/accessible/root";
/// The most nodes one walk visits, and how deep it goes: a large window still answers in bounded time.
const MAX_NODES: usize = 1_000;
const MAX_DEPTH: usize = 40;
/// The most characters of a text value read.
const MAX_TEXT_CHARS: i32 = 2_000;

/// The session's accessibility bus.
pub struct AtspiTree {
    bus: Connection,
    /// How far down each element of the last walk is drawn from where it reports itself (what's under `LibreOffice`'s
    /// root pane, [`dock_step`]), so a click at its box later lands where its mark is.
    shifts: Mutex<HashMap<String, i32>>,
}

/// A node the walk has visited: its role, its parent's handle and where the walk placed it.
struct Seen {
    role: String,
    parent: String,
    place: Place,
}

/// A node waiting in the walk: its bus name and path, its depth, its parent's handle and where it stands with respect
/// to a shifted root pane.
struct Pending {
    name: String,
    path: String,
    depth: usize,
    parent: String,
    dock: Dock,
}

impl AtspiTree {
    /// Connects to the accessibility bus the session bus names (`DBUS_SESSION_BUS_ADDRESS`, from the image's init).
    ///
    /// # Errors
    /// `Unavailable` when there is no session bus or no accessibility bus on it.
    pub async fn connect() -> Result<Self, DriverError> {
        let session = Connection::session().await.map_err(unavailable)?;
        let address = BusProxy::new(&session)
            .await
            .map_err(unavailable)?
            .get_address()
            .await
            .map_err(unavailable)?;
        Self::connect_to(&address).await
    }

    /// Connects to the accessibility bus at `address`.
    ///
    /// # Errors
    /// `Unavailable` when nothing answers there.
    pub async fn connect_to(address: &str) -> Result<Self, DriverError> {
        let bus = zbus::connection::Builder::address(address)
            .build()
            .await
            .map_err(unavailable)?;
        Ok(Self {
            bus,
            shifts: Mutex::new(HashMap::new()),
        })
    }

    /// A proxy of interface `T` for the object at `path` of the application at `name`, reading properties afresh.
    /// A name or path that isn't one names no object: it is gone.
    async fn proxy<T>(&self, name: &str, path: &str) -> Result<T, DriverError>
    where
        T: From<zbus::Proxy<'static>> + Defaults,
    {
        BusName::try_from(name).map_err(|_| DriverError::Gone)?;
        ObjectPath::try_from(path).map_err(|_| DriverError::Gone)?;
        Builder::<T>::new(&self.bus)
            .destination(name.to_owned())
            .path(path.to_owned())
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(unavailable)
    }

    /// The process that owns a bus name on the accessibility bus.
    async fn pid(&self, name: &str) -> Option<u32> {
        let dbus = zbus::fdo::DBusProxy::new(&self.bus).await.ok()?;
        let name = BusName::try_from(name.to_owned()).ok()?;
        dbus.get_connection_unix_process_id(name).await.ok()
    }

    /// The window of `app` that is active, else the first one showing.
    async fn active_window(
        &self,
        app: &AccessibleProxy<'_>,
    ) -> Result<Option<(String, String)>, DriverError> {
        let mut showing = None;
        let children = app.get_children().await.map_err(unavailable)?;
        for (name, path) in objects(&children) {
            let frame: AccessibleProxy<'_> = self.proxy(name, path).await?;
            let words = frame.get_state().await.map_err(unavailable)?;
            if has(&words, "active") {
                return Ok(Some((name.to_owned(), path.to_owned())));
            }
            if showing.is_none() && has(&words, "showing") {
                showing = Some((name.to_owned(), path.to_owned()));
            }
        }
        Ok(showing)
    }

    /// The window's nodes in reading order, depth first, within [`MAX_NODES`] and [`MAX_DEPTH`], skipping subtrees
    /// that aren't showing. What's under `LibreOffice`'s root pane moves down to where it is drawn, and its hidden menu
    /// bar is left out ([`dock_step`]); a thumbnail view's items move up to where it draws them
    /// ([`thumbnail_offset`]); the shifts are kept for the elements' extents read later.
    async fn walk(&self, name: &str, path: &str) -> Result<Vec<Node>, DriverError> {
        let mut nodes = Vec::new();
        let mut shifts = HashMap::new();
        // Each node's role, parent and place, and each listed node's parent: for the thumbnail views after the walk.
        let mut seen: HashMap<String, Seen> = HashMap::new();
        let mut parents = Vec::new();
        let mut pending = vec![Pending {
            name: name.to_owned(),
            path: path.to_owned(),
            depth: 0,
            parent: String::new(),
            dock: Dock::Out,
        }];
        let mut visited = 0;
        while let Some(Pending {
            name,
            path,
            depth,
            parent,
            dock,
        }) = pending.pop()
        {
            visited += 1;
            if visited > MAX_NODES {
                break;
            }
            let Ok(proxy) = self.proxy::<AccessibleProxy<'_>>(&name, &path).await else {
                continue;
            };
            let Ok(words) = proxy.get_state().await else {
                continue;
            };
            if depth > 0 && !has(&words, "showing") {
                continue;
            }
            let role = proxy.get_role_name().await.unwrap_or_default();
            let handle = format!("{name}{path}");
            let root_pane = role == "root pane";
            // Only where a root pane may shift its nodes does the walk need every node's place.
            let extents = if root_pane || matches!(dock, Dock::Shifted { .. }) {
                self.reported_extents(&handle).await.ok().flatten()
            } else {
                None
            };
            let root_pane_shift = if root_pane {
                self.root_pane_shift(&parent, extents).await
            } else {
                None
            };
            let (place, inside) = dock_step(dock, &role, extents, root_pane_shift);
            if place == Place::Hidden {
                continue;
            }
            seen.insert(
                handle.clone(),
                Seen {
                    role: role.clone(),
                    parent: parent.clone(),
                    place,
                },
            );
            if listed(&role) {
                let node = self.node(&proxy, (&name, &path), role, &words).await;
                nodes.push(shifted(node, place, &mut shifts));
                parents.push(parent);
            }
            if depth < MAX_DEPTH {
                let children = proxy.get_children().await.unwrap_or_default();
                push_children(&mut pending, &children, depth + 1, &handle, inside);
            }
        }
        self.place_thumbnails(&mut nodes, &parents, &seen, &mut shifts)
            .await;
        *self
            .shifts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = shifts;
        Ok(nodes)
    }

    /// Moves each thumbnail view's items (`list item`s in a `list` under a `viewport` under a `scroll pane`) to where
    /// the view draws them, by [`thumbnail_offset`] from the list's own place, and keeps the shift for a click.
    async fn place_thumbnails(
        &self,
        nodes: &mut [Node],
        parents: &[String],
        seen: &HashMap<String, Seen>,
        shifts: &mut HashMap<String, i32>,
    ) {
        let role_of = |handle: &str| {
            seen.get(handle)
                .map(|found| (found.role.as_str(), found.parent.as_str()))
        };
        let mut views: Vec<&str> = Vec::new();
        for (node, parent) in nodes.iter().zip(parents) {
            let Some(("list", viewport)) = role_of(parent) else {
                continue;
            };
            let Some(("viewport", pane)) = role_of(viewport) else {
                continue;
            };
            if node.role == "list item"
                && matches!(role_of(pane), Some(("scroll pane", _)))
                && !views.contains(&parent.as_str())
            {
                views.push(parent);
            }
        }
        for view in views {
            let shift = match seen.get(view).map(|found| found.place) {
                Some(Place::Moved(shift)) => shift,
                _ => 0,
            };
            let Ok(Some((x, y, width, height))) = self.reported_extents(view).await else {
                continue;
            };
            let members: Vec<usize> = (0..nodes.len())
                .filter(|&index| parents[index] == view && nodes[index].role == "list item")
                .collect();
            let items: Vec<_> = members
                .iter()
                .filter_map(|&index| nodes[index].extents)
                .collect();
            let Some(offset) = thumbnail_offset((x, y + shift, width, height), &items) else {
                continue;
            };
            for index in members {
                let node = &mut nodes[index];
                node.extents = node
                    .extents
                    .map(|(x, y, width, height)| (x, y - offset, width, height));
                *shifts.entry(node.handle.clone()).or_insert(0) -= offset;
            }
        }
    }

    /// How far a root pane is reported above its parent ([`dock_shift`]), when both say where they are.
    async fn root_pane_shift(
        &self,
        parent: &str,
        root_pane: Option<(i32, i32, i32, i32)>,
    ) -> Option<i32> {
        let (_, parent_y, _, _) = self.reported_extents(parent).await.ok().flatten()?;
        dock_shift(parent_y, root_pane?.1)
    }

    /// An element's extents as the toolkit reports them, before any shift.
    async fn reported_extents(
        &self,
        handle: &str,
    ) -> Result<Option<(i32, i32, i32, i32)>, DriverError> {
        let component = self.component(handle).await?;
        match component.get_extents(SCREEN).await {
            Ok((x, y, width, height)) if width > 0 && height > 0 => Ok(Some((x, y, width, height))),
            Ok(_) => Ok(None),
            Err(error) => Err(gone_or_unavailable(error)),
        }
    }

    async fn node(
        &self,
        proxy: &AccessibleProxy<'_>,
        (name, path): (&str, &str),
        role: String,
        words: &[u32],
    ) -> Node {
        let value = if role == "password text" {
            None
        } else {
            self.value(proxy, name, path).await
        };
        let handle = format!("{name}{path}");
        // A node that can't say where it is keeps its ref and gets no mark.
        let extents = self.reported_extents(&handle).await.ok().flatten();
        Node {
            role,
            name: shown_name(
                &proxy.name().await.unwrap_or_default(),
                &proxy.description().await.unwrap_or_default(),
            ),
            states: states(words),
            value,
            handle,
            extents,
        }
    }

    /// Its text, or its numeric value, when it has one.
    async fn value(&self, proxy: &AccessibleProxy<'_>, name: &str, path: &str) -> Option<String> {
        let interfaces = proxy.get_interfaces().await.ok()?;
        if interfaces.iter().any(|interface| interface == TEXT) {
            let text: TextProxy<'_> = self.proxy(name, path).await.ok()?;
            let count = text.character_count().await.ok()?.min(MAX_TEXT_CHARS);
            return text.get_text(0, count).await.ok();
        }
        if interfaces.iter().any(|interface| interface == VALUE) {
            let value: ValueProxy<'_> = self.proxy(name, path).await.ok()?;
            return value
                .current_value()
                .await
                .ok()
                .map(|number| number.to_string());
        }
        None
    }

    async fn component(&self, handle: &str) -> Result<ComponentProxy<'static>, DriverError> {
        let (name, path) = split(handle)?;
        self.proxy(name, path).await
    }
}

#[async_trait]
impl A11y for AtspiTree {
    async fn window(&self, pid: u32) -> Result<Option<Window>, DriverError> {
        let root: AccessibleProxy<'_> = self.proxy(REGISTRY, ROOT).await?;
        let apps = root.get_children().await.map_err(unavailable)?;
        for (name, path) in objects(&apps) {
            if self.pid(name).await != Some(pid) {
                continue;
            }
            let app: AccessibleProxy<'_> = self.proxy(name, path).await?;
            let Some((window_name, window_path)) = self.active_window(&app).await? else {
                return Ok(None);
            };
            let frame: AccessibleProxy<'_> = self.proxy(&window_name, &window_path).await?;
            let title = frame.name().await.unwrap_or_default();
            let nodes = self.walk(&window_name, &window_path).await?;
            return Ok(Some(Window {
                handle: format!("{window_name}{window_path}"),
                title,
                nodes,
            }));
        }
        Ok(None)
    }

    async fn act(&self, handle: &str, names: &[&str]) -> Result<bool, DriverError> {
        let (name, path) = split(handle)?;
        let action: ActionProxy<'_> = self.proxy(name, path).await?;
        let Ok(actions) = action.get_actions().await else {
            return Ok(false);
        };
        let shown: Vec<String> = actions.into_iter().map(|(name, _, _)| name).collect();
        let mut own = Vec::with_capacity(shown.len());
        for index in 0..shown.len() {
            let index =
                i32::try_from(index).map_err(|_| DriverError::Failed("too many actions".into()))?;
            own.push(action.get_name(index).await.ok());
        }
        let Some(index) = action_index(&own, &shown, names) else {
            return Ok(false);
        };
        let index =
            i32::try_from(index).map_err(|_| DriverError::Failed("too many actions".into()))?;
        action.do_action(index).await.map_err(gone_or_unavailable)
    }

    async fn set_text(&self, handle: &str, text: &str) -> Result<bool, DriverError> {
        let (name, path) = split(handle)?;
        let editable: EditableTextProxy<'_> = self.proxy(name, path).await?;
        Ok(editable.set_text_contents(text).await.unwrap_or(false))
    }

    async fn focus(&self, handle: &str) -> Result<bool, DriverError> {
        let component = self.component(handle).await?;
        Ok(component.grab_focus().await.unwrap_or(false))
    }

    async fn extents(&self, handle: &str) -> Result<Option<(i32, i32, i32, i32)>, DriverError> {
        let shift = self
            .shifts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(handle)
            .copied()
            .unwrap_or(0);
        Ok(self
            .reported_extents(handle)
            .await?
            .map(|(x, y, width, height)| (x, y + shift, width, height)))
    }
}

/// The objects a `GetChildren` reply names, as (bus name, path), in order. A null reference is no object, whatever
/// bus name it carries, so nothing asks it for its state (GTK's bridge sends its own name with the null path).
pub(super) fn objects(children: &[(String, zbus::OwnedObjectPath)]) -> Vec<(&str, &str)> {
    children
        .iter()
        .filter_map(|(name, path)| named(name, path.as_str()).map(|name| (name, path.as_str())))
        .collect()
}

/// The node at its place, a shift kept for its extents read later.
fn shifted(mut node: Node, place: Place, shifts: &mut HashMap<String, i32>) -> Node {
    if let Place::Moved(shift) = place {
        node.extents = node
            .extents
            .map(|(x, y, width, height)| (x, y + shift, width, height));
        shifts.insert(node.handle.clone(), shift);
    }
    node
}

/// The children onto the walk's stack, reversed, so the walk reads them in order, each with its parent's handle and
/// the `Dock` its parent gives it.
fn push_children(
    pending: &mut Vec<Pending>,
    children: &[(String, zbus::OwnedObjectPath)],
    depth: usize,
    parent: &str,
    dock: Dock,
) {
    for (name, path) in objects(children).into_iter().rev() {
        pending.push(Pending {
            name: name.to_owned(),
            path: path.to_owned(),
            depth,
            parent: parent.to_owned(),
            dock,
        });
    }
}

/// A handle's bus name and object path: `:1.23/org/a11y/atspi/accessible/42`.
pub(super) fn split(handle: &str) -> Result<(&str, &str), DriverError> {
    let at = handle.find('/').ok_or(DriverError::Gone)?;
    Ok(handle.split_at(at))
}

fn unavailable(_: zbus::Error) -> DriverError {
    DriverError::Unavailable("the accessibility bus isn't answering".into())
}

/// An object the application has dropped is gone; anything else is the bus. zbus 6 reports the standard D-Bus errors
/// as `FDO`, and any other as a `MethodError` by name.
pub(super) fn gone_or_unavailable(error: zbus::Error) -> DriverError {
    let gone = match &error {
        zbus::Error::FDO(standard) => matches!(
            **standard,
            zbus::fdo::Error::UnknownObject(_) | zbus::fdo::Error::UnknownMethod(_)
        ),
        zbus::Error::MethodError(name, _, _) => unknown(name.as_str()),
        _ => false,
    };
    if gone {
        DriverError::Gone
    } else {
        unavailable(error)
    }
}

/// Whether a D-Bus error name says the object or its method isn't there.
pub(super) fn unknown(error_name: &str) -> bool {
    error_name.ends_with("UnknownObject") || error_name.ends_with("UnknownMethod")
}
