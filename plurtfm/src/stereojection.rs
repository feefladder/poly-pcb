use three_d::{InnerSpace, Mat4, Quat, Vec3};

/// A stereographic projection
/// ```text
/// +-----^   ^
///  \    |   |
///   +---+   |  arrow * dist
///    \  |   |
///     \ |   |
///      point
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Stereojection {
    /// the point from which everything is viewed
    pub point: Vec3,
    /// normalized direction to plane
    pub arrow: Vec3,
    /// The distance to the plane, or arrow length
    pub dist: f32,
}

impl Stereojection {
    pub fn project(&self, t: Mat4) -> Mat4 {
        //make w on the plane
        //or ehh...
        // ok, so we basically have some similar triangles
        // +-----^      ^
        //  \    |      |
        // p +---+      | dist
        //    \  |dir.a |
        // dir \ |      |
        //      point
        // and then...
        // feels like we need some dot-product thingies
        let p = t.w.truncate();
        let dir = self.point - p;
        let a = self.arrow;
        let x = t.x.truncate();
        let y = t.y.truncate();
        let z = t.z.truncate();
        // project
        if a.dot(z).signum() != -1.0 {
            return t;
        }
        let zp = -a;
        let q = Quat::from_arc(z, zp, None);
        let xp = q * x;
        let yp = q * y;
        // if 1.0 - a.dot(x).abs() < 1e-8 {
        //     yp = (y - a * a.dot(y)).normalize();
        //     xp = yp.cross(zp);
        // } else {
        //     xp = (x - a * a.dot(x)).normalize();
        //     yp = zp.cross(xp);
        // }

        Mat4::from_cols(
            // transform is just orthograpically aligned to plane
            xp.extend(0.0),
            yp.extend(0.0),
            zp.extend(0.0),
            // this is similar triangles from point
            (self.point + dir * (self.dist / dir.dot(a))).extend(1.0),
        )
    }
}
