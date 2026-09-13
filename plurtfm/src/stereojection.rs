//! Ok, so I guess it's blurb time!
//!
//! what's the deal with a stereojection?
//! so a stereojection is like a double stereographic projection
//! some todos (in no real particular order):
//! 1. have a directly mirrored projection
//! 2. get like the inner radii + outer radii of all faces that are adjacent to the center of projection and adjust distance based on that
//! 3. do the flippy thingy
//!
//! The flippy thingy is I think kinda easy-ish:
//! 1. parent the mirrored transform to the first (to be defined) pcb.
//! 2. determine both projected transforms of the first pcb
//! 3. get the movement thingy from the difference of those two??
//! 4. also move the projection arrow
//! 5. parent all other mirror-projected pcbs to the projection arrow
//! 6. transform those according to that
//!
//! ok, maybe not entirely easy, but certainly doable!
//!
//! here's great links:
//! Ahh https://community.khronos.org/t/understanding-child-parent-transformation/74289/4
//! and also kinda so
//!
//! parenting is basically finding the relative transform, because normally we just have `T`
//! but then we want `A*T'` to be `T`, so `AT'=T => T' = inv(A)T`
//!
//! okidoki
//!
//! and for kinda this reason, we also want the projection to have a transform
//! just so we have a set of axes to construct A
//!
//! and to easify from-to-transform. Because the projected thing is also weird?
//! No it's not weird, can just project points, but need to have axis
//!

// Ok, also mandatory journal-session reflection on the course this week
//
// So there was this like thingy that was the stick-moving, where D found out
// that it's much easier to keep contact when you're crossed-arms because then
// there's much more leeway. en nu ffkes nederlands. Dus dat is een reflectiepunt. Ander reflectiepunt is...
// naja het vitrinekast-ding, en msschien wat andere belangrijke dingen?
//
// ehh owja, er gaan wat dingen door elkaar heen van de twee vakken, maar wat Luuk (docent) zei over "je wereld vanuit het vak zien, ipv het vak vanuit je wereld" of eigg was het over "denken vanuit daar of daar", maar daar gaat het nu niet om. Waar het wel om draait, dat weet natuurlijk niemand, en wat ik wilde zeggen is dat het dus ook heel leuk was dat eigenlijk vanuit het stok-spel, dat deze hele projectiezooi daar best een beetje op lijkt zegmaar. Dus toen ging ik met het project-ding dansen net als hoe ik ook danste met mensen en stokken. Dat staat ook in de tekening, en dit zijn dus de bijbehorende aantekeningen lol. Nou.. Ja het leiderschapsding. Daar ben ik nu dus denk ik ook een beetje mn eigen ding aan het doen op werk ofzo, maar yufeng is dus eigenlijk mn eerste volger, (en msschien zou t echt wel leuk zijn om met Barnes te gaan praten). En er lopen wel paar dingetjes synchroon aan elkaar, waardoor ik nu zometeen vet veel videos moet maken, en ja dat is best mooi denk ik.
//
// En nu t toch op video staat ook ffkes zeggen dat het kleurpatroon van mn IDE precies een grond is. En daarin zit ook half het ding, want het hele toekomstboeren-ding en "maar het is een platform"-shenanigans (moet positivere benaming vinden van gezeur)

use three_d::{InnerSpace, Mat3, Mat4, One, Quat, Vec3, VectorSpace};

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
    ///
    /// So actually this is like the basis
    /// with ehh... Z axis as the arrow direction
    pub arrow: Mat3,
    /// The distance to the plane, or arrow length
    pub dist: f32,
}

impl Stereojection {
    pub fn project(&self, t: Mat4, amount: f32) -> Mat4 {
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
        let a = self.arrow.z;
        let x = t.x.truncate();
        let y = t.y.truncate();
        let z = t.z.truncate();

        let zp = -a;
        // so the minimum rotation to get original z to align with projected z
        let q = Quat::from_arc(z, zp, None);
        let rot = Quat::one().slerp(q, amount);
        let xp = rot * x;
        let yp = rot * y;
        let zp = rot * z;
        // I guess we can do the thingythingy where...
        // so need to get og rot, which is simple
        let wp = p.lerp(self.point + dir * (self.dist / dir.dot(a)), amount);

        Mat4::from_cols(
            xp.extend(0.0),
            yp.extend(0.0),
            zp.extend(0.0),
            // this is similar triangles from point
            wp.extend(1.0),
        )
    }
}
