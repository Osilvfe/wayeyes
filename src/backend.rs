pub mod image_copy;
pub mod portal;

use std::{sync::mpsc::{self, Receiver}, thread};

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
}

impl CursorModel {
    pub fn set_local(&mut self, point: Point) {
        self.local = Some(point);
        self.try_calibrate();
    }

    pub fn set_global(&mut self, point: Point) {
        self.global = Some(point);
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
}
