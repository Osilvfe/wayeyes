use std::{cell::RefCell, f64::consts::TAU, rc::Rc, time::Duration};

use gtk::glib::{self, ControlFlow};
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, DrawingArea, EventControllerMotion};
use tracing::{info, warn};

use crate::backend::{BackendEvent, CursorModel, portal};
use crate::cli::{BackendKind, Cli};
use crate::geometry::{Point, eye_pair, pupil_center};

const APP_ID: &str = "io.github.osilvfe.wayeyes";

pub fn run(cli: Cli) -> anyhow::Result<()> {
    let app = Application::builder().application_id(APP_ID).build();

    app.connect_activate(move |app| build_ui(app, &cli));

    // clap owns WayEyes' CLI. Do not let GApplication parse the process arguments
    // a second time, otherwise options such as `--backend` are rejected by GTK.
    app.run_with_args(&["wayeyes"]);
    Ok(())
}

fn build_ui(app: &Application, cli: &Cli) {
    info!(backend = ?cli.backend, "starting WayEyes");

    let cursor = Rc::new(RefCell::new(CursorModel::default()));
    let area = DrawingArea::builder().hexpand(true).vexpand(true).build();

    {
        let cursor = Rc::clone(&cursor);
        area.set_draw_func(move |_area, cr, width, height| {
            draw_eyes(
                cr,
                width as f64,
                height as f64,
                cursor.borrow().target_in_surface(),
            );
        });
    }

    let motion = EventControllerMotion::new();
    {
        let cursor = Rc::clone(&cursor);
        let area = area.clone();
        motion.connect_motion(move |_controller, x, y| {
            cursor.borrow_mut().set_local(Point::new(x, y));
            area.queue_draw();
        });
    }
    area.add_controller(motion);

    if matches!(cli.backend, BackendKind::Auto | BackendKind::Portal) {
        attach_backend(portal::spawn(), Rc::clone(&cursor), area.clone());
    }

    let window = ApplicationWindow::builder()
        .application(app)
        .title("WayEyes")
        .default_width(cli.width.max(120))
        .default_height(cli.height.max(80))
        .child(&area)
        .build();

    if cli.undecorated {
        window.set_decorated(false);
    }

    window.present();
}

fn attach_backend(
    receiver: std::sync::mpsc::Receiver<BackendEvent>,
    cursor: Rc<RefCell<CursorModel>>,
    area: DrawingArea,
) {
    glib::timeout_add_local(Duration::from_millis(8), move || {
        while let Ok(event) = receiver.try_recv() {
            match event {
                BackendEvent::Ready { backend, streams } => {
                    info!(backend, streams, "pointer backend ready");
                }
                BackendEvent::Pointer(point) => {
                    cursor.borrow_mut().set_global(point);
                    area.queue_draw();
                }
                BackendEvent::Failed { backend, message } => {
                    warn!(backend, %message, "pointer backend failed; local tracking remains available");
                }
            }
        }

        ControlFlow::Continue
    });
}

fn draw_eyes(cr: &gtk::cairo::Context, width: f64, height: f64, target: Option<Point>) {
    let eyes = eye_pair(width, height);
    let target = target.unwrap_or(Point::new(width * 0.5, height * 0.5));

    cr.set_source_rgb(0.96, 0.96, 0.96);
    let _ = cr.paint();

    for eye in eyes {
        let _ = cr.save();
        cr.translate(eye.center.x, eye.center.y);
        cr.scale(eye.radius_x, eye.radius_y);
        cr.arc(0.0, 0.0, 1.0, 0.0, TAU);
        let _ = cr.restore();

        cr.set_source_rgb(1.0, 1.0, 1.0);
        let _ = cr.fill_preserve();
        cr.set_source_rgb(0.08, 0.08, 0.08);
        cr.set_line_width(2.0);
        let _ = cr.stroke();

        let pupil = pupil_center(eye, target);
        cr.arc(pupil.x, pupil.y, eye.pupil_radius, 0.0, TAU);
        cr.set_source_rgb(0.05, 0.05, 0.05);
        let _ = cr.fill();
    }
}
