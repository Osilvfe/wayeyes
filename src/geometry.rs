#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EyeGeometry {
    pub center: Point,
    pub radius_x: f64,
    pub radius_y: f64,
    pub pupil_radius: f64,
}

pub fn eye_pair(width: f64, height: f64) -> [EyeGeometry; 2] {
    let width = width.max(1.0);
    let height = height.max(1.0);
    let radius_x = (width * 0.19).min(height * 0.36).max(8.0);
    let radius_y = (height * 0.38).min(width * 0.24).max(10.0);
    let pupil_radius = radius_x.min(radius_y) * 0.34;

    [
        EyeGeometry {
            center: Point::new(width * 0.30, height * 0.50),
            radius_x,
            radius_y,
            pupil_radius,
        },
        EyeGeometry {
            center: Point::new(width * 0.70, height * 0.50),
            radius_x,
            radius_y,
            pupil_radius,
        },
    ]
}

pub fn pupil_center(eye: EyeGeometry, target: Point) -> Point {
    let travel_x = (eye.radius_x - eye.pupil_radius - 3.0).max(0.0);
    let travel_y = (eye.radius_y - eye.pupil_radius - 3.0).max(0.0);

    if travel_x == 0.0 || travel_y == 0.0 {
        return eye.center;
    }

    let dx = target.x - eye.center.x;
    let dy = target.y - eye.center.y;
    let norm = (dx * dx / (travel_x * travel_x) + dy * dy / (travel_y * travel_y)).sqrt();

    if norm <= 1.0 || norm == 0.0 {
        target
    } else {
        Point::new(eye.center.x + dx / norm, eye.center.y + dy / norm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_inside_travel_ellipse_is_not_clamped() {
        let eye = EyeGeometry {
            center: Point::new(50.0, 50.0),
            radius_x: 30.0,
            radius_y: 40.0,
            pupil_radius: 10.0,
        };
        let target = Point::new(55.0, 60.0);
        assert_eq!(pupil_center(eye, target), target);
    }

    #[test]
    fn distant_target_is_clamped() {
        let eye = EyeGeometry {
            center: Point::new(50.0, 50.0),
            radius_x: 30.0,
            radius_y: 40.0,
            pupil_radius: 10.0,
        };
        let pupil = pupil_center(eye, Point::new(500.0, 50.0));
        assert!((pupil.x - 67.0).abs() < 0.001);
        assert!((pupil.y - 50.0).abs() < 0.001);
    }
}
