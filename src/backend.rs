pub mod image_copy;
pub mod portal;

use std::{
    sync::mpsc::{self, Receiver},
    thread,
};

use crate::geometry::Point;

#[derive(Debug, Clone)]
pub enum BackendEvent {
    Ready {
        backend: &'static str,
        streams: usize,
    },
    Pointer(Point),
    Failed {
        backend: &'static str,
        message: String,
    },
}

pub fn spawn_auto() -> Receiver<BackendEvent> {
    let (tx, rx) = mpsc::channel();

    thread::Builder::new()
        .name("wayeyes-backend-auto".into())
        .spawn(move || {
            let direct = image_copy::spawn();
            let mut direct_ready = false;

            while let Ok(event) = direct.recv() {
                match &event {
                    BackendEvent::Ready { .. } => direct_ready = true,
                    BackendEvent::Failed { .. } if !direct_ready => {
                        let _ = tx.send(event);
                        break;
                    }
                    BackendEvent::Failed { .. } => {
                        let _ = tx.send(event);
                        return;
                    }
                    _ => {}
                }

                if tx.send(event).is_err() {
                    return;
                }
            }

            // Direct Wayland was unavailable before becoming ready. Try the
            // portal backend; if that also fails, the GTK-local path remains.
            let portal = portal::spawn();
            while let Ok(event) = portal.recv() {
                if tx.send(event).is_err() {
                    return;
                }
            }
        })
        .expect("failed to spawn backend selection thread");

    rx
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CursorModel {
    local: Option<Point>,
    global: Option<Point>,
    surface_origin: Option<Point>,
    recalibrate_on_next_global: bool,
}

impl CursorModel {
    pub fn set_local(&mut self, point: Point) {
        self.local = Some(point);
        self.try_calibrate();
    }

    /// Record a fresh local sample from a pointer-enter event.
    ///
    /// The global backend is delivered on another event loop, so the newest
    /// global point may arrive a few milliseconds after GTK's enter signal.
    /// Calibrate immediately for responsiveness, then calibrate once more from
    /// the next global sample. Ordinary global updates never reuse stale local
    /// coordinates.
    pub fn set_local_on_enter(&mut self, point: Point) {
        self.local = Some(point);
        self.try_calibrate();
        self.recalibrate_on_next_global = true;
    }

    pub fn set_global(&mut self, point: Point) {
        self.global = Some(point);
        if self.recalibrate_on_next_global {
            self.try_calibrate();
            self.recalibrate_on_next_global = false;
        }
    }

    pub fn target_in_surface(&self) -> Option<Point> {
        match (self.global, self.surface_origin) {
            (Some(global), Some(origin)) => {
                Some(Point::new(global.x - origin.x, global.y - origin.y))
            }
            _ => self.local,
        }
    }

    fn try_calibrate(&mut self) {
        if let (Some(global), Some(local)) = (self.global, self.local) {
            self.surface_origin = Some(Point::new(global.x - local.x, global.y - local.y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_coordinates_work_without_global_backend() {
        let mut model = CursorModel::default();
        model.set_local(Point::new(12.0, 34.0));
        assert_eq!(model.target_in_surface(), Some(Point::new(12.0, 34.0)));
    }

    #[test]
    fn local_motion_calibrates_surface_origin_once_global_is_known() {
        let mut model = CursorModel::default();
        model.set_global(Point::new(420.0, 230.0));
        model.set_local(Point::new(20.0, 30.0));

        model.set_global(Point::new(500.0, 260.0));

        assert_eq!(model.target_in_surface(), Some(Point::new(100.0, 60.0)));
    }

    #[test]
    fn ordinary_global_updates_do_not_reuse_stale_local_coordinates() {
        let mut model = CursorModel::default();
        model.set_global(Point::new(420.0, 230.0));
        model.set_local(Point::new(20.0, 30.0));

        model.set_global(Point::new(500.0, 260.0));
        model.set_global(Point::new(520.0, 270.0));

        assert_eq!(model.target_in_surface(), Some(Point::new(120.0, 70.0)));
    }

    #[test]
    fn pointer_enter_recalibrates_again_on_next_global_sample() {
        let mut model = CursorModel::default();
        model.set_global(Point::new(420.0, 230.0));
        model.set_local(Point::new(20.0, 30.0));

        // Imagine the compositor moved the WayEyes window while the pointer was
        // away. GTK sees the pointer enter first, then the direct backend sends
        // the matching fresh global point.
        model.set_local_on_enter(Point::new(10.0, 15.0));
        model.set_global(Point::new(710.0, 415.0));
        model.set_global(Point::new(730.0, 425.0));

        assert_eq!(model.target_in_surface(), Some(Point::new(30.0, 25.0)));
    }
}
