//! JOML, the game's vector and matrix library, as the translator takes it from the player's
//! install: a fixture that uses it runs on `java` with the install's jar, then is translated
//! with that jar's own code and run again; every number must match bit for bit.
//! Needs a JDK (`javac`, `java`, `jar`) and a NeoForge install; skipped without either.
use std::{fs, path::Path, process::Command};

/// (method, inputs): each is `static long name(int)`.
const CASES: &[(&str, &[&str])] = &[
    ("vectors", &["0", "3", "17"]),
    ("doubles", &["1", "9"]),
    ("quaternions", &["0", "2", "11"]),
    ("matrices", &["0", "1", "5", "8"]),
    ("properties", &["0", "4"]),
    ("inverses", &["1", "6"]),
    ("projections", &["2", "7"]),
    ("matrix3", &["1", "4"]),
    ("stack", &["3"]),
    ("arrays", &["2"]),
    ("matrix4d", &["1", "5"]),
    ("identities", &["0"]),
];

/// The fixture: digests of what JOML computes, so one number says every component matched.
const FIXTURE: &str = r#"
import org.joml.AxisAngle4f;
import org.joml.Matrix3d;
import org.joml.Matrix3f;
import org.joml.Matrix4d;
import org.joml.Matrix4f;
import org.joml.Matrix4fStack;
import org.joml.Matrix4x3f;
import org.joml.Quaterniond;
import org.joml.Quaternionf;
import org.joml.Vector2f;
import org.joml.Vector3d;
import org.joml.Vector3f;
import org.joml.Vector3i;
import org.joml.Vector4f;

public class JomlFixture {
    static long h(long acc, float f) { return acc * 31 + Float.floatToRawIntBits(f); }
    static long h(long acc, double d) { return acc * 31 + Double.doubleToRawLongBits(d); }
    static long h(long acc, Vector3f v) { return h(h(h(acc, v.x), v.y), v.z); }
    static long h(long acc, Vector3d v) { return h(h(h(acc, v.x), v.y), v.z); }
    static long h(long acc, Vector4f v) { return h(h(h(h(acc, v.x), v.y), v.z), v.w); }
    static long h(long acc, Quaternionf q) { return h(h(h(h(acc, q.x), q.y), q.z), q.w); }
    static long h(long acc, Quaterniond q) { return h(h(h(h(acc, q.x), q.y), q.z), q.w); }
    static long h(long acc, Matrix4f m) {
        float[] a = m.get(new float[16]);
        for (float f : a) acc = h(acc, f);
        return acc * 31 + m.properties();
    }
    static long h(long acc, Matrix3f m) {
        float[] a = m.get(new float[9]);
        for (float f : a) acc = h(acc, f);
        return acc;
    }
    static long h(long acc, Matrix4d m) {
        double[] a = m.get(new double[16]);
        for (double d : a) acc = h(acc, d);
        return acc * 31 + m.properties();
    }

    public static long vectors(int n) {
        long acc = 7;
        Vector3f a = new Vector3f(1.5f + n, -2.25f, 0.125f * n);
        Vector3f b = new Vector3f(n, 3, -4.5f);
        acc = h(acc, new Vector3f(a).add(b));
        acc = h(acc, new Vector3f(a).sub(b).mul(1.75f));
        acc = h(acc, a.cross(b, new Vector3f()));
        acc = h(acc, a.dot(b));
        acc = h(acc, a.length());
        acc = h(acc, a.distance(b));
        acc = h(acc, new Vector3f(a).normalize());
        acc = h(acc, new Vector3f(a).lerp(b, 0.3f));
        acc = h(acc, new Vector3f(a).min(b).max(new Vector3f(0.5f)));
        acc = h(acc, new Vector3f(a).negate().absolute());
        acc = h(acc, new Vector3f(a).fma(2.5f, b));
        acc = h(acc, new Vector3f(a).rotateY(0.75f + n).rotateX(-0.5f).rotateZ(1.25f));
        acc = h(acc, new Vector3f(a).rotateAxis(0.6f, 0.0f, 0.6f, 0.8f));
        acc = h(acc, a.angle(b));
        acc = h(acc, new Vector3f(a).reflect(new Vector3f(0, 1, 0)));
        acc = h(acc, new Vector3f(a).div(3.0f).mul(b));
        Vector4f four = new Vector4f(a, 1.0f);
        acc = h(acc, four.mul(new Matrix4f().translate(1, 2, 3).rotateY(0.5f)));
        acc = h(acc, new Vector4f(1, n, 3, 0.5f).normalize());
        Vector2f two = new Vector2f(n + 0.5f, -1.5f);
        acc = h(acc, two.length());
        acc = h(acc, two.normalize().perpendicular().x);
        Vector3i whole = new Vector3i(n, 4, -9).mul(3).add(1, 2, 3);
        acc = acc * 31 + whole.x + 7L * whole.y + 49L * whole.z + whole.lengthSquared();
        acc = acc * 31 + (a.equals(new Vector3f(a)) ? 1 : 0) + (a.equals(b) ? 2 : 0) + a.hashCode();
        acc = acc * 31 + a.maxComponent() + 3 * b.minComponent();
        return acc;
    }

    public static long doubles(int n) {
        long acc = 11;
        Vector3d a = new Vector3d(0.1 * n, 2.5, -7.75);
        Vector3d b = new Vector3d(4, -n, 0.3);
        acc = h(acc, new Vector3d(a).add(b).mul(0.7));
        acc = h(acc, a.cross(b, new Vector3d()));
        acc = h(acc, a.dot(b));
        acc = h(acc, a.length());
        acc = h(acc, new Vector3d(a).normalize());
        acc = h(acc, new Vector3d(a).lerp(b, 0.25));
        acc = h(acc, new Vector3d(a).rotateY(0.3 * n));
        acc = h(acc, a.distanceSquared(b));
        Quaterniond q = new Quaterniond().rotateXYZ(0.2, 0.4 * n, -0.6);
        acc = h(acc, q);
        acc = h(acc, q.transform(new Vector3d(a)));
        acc = h(acc, new Quaterniond(q).mul(new Quaterniond().rotateY(1.1)).normalize());
        acc = h(acc, new Quaterniond(q).slerp(new Quaterniond().rotateZ(2.0), 0.4));
        acc = h(acc, new Quaterniond(q).invert());
        acc = h(acc, q.getEulerAnglesXYZ(new Vector3d()));
        acc = h(acc, new Vector3d(a).mulPosition(new Matrix4d().translate(1, 2, 3).rotateX(0.5 + n)));
        acc = h(acc, new Vector3f().set(a));
        return acc;
    }

    public static long quaternions(int n) {
        long acc = 13;
        Quaternionf q = new Quaternionf().rotateXYZ(0.3f, 0.2f * n, -0.9f);
        acc = h(acc, q);
        acc = h(acc, new Quaternionf().rotationXYZ(0.1f * n, 0.5f, 0.25f));
        acc = h(acc, new Quaternionf().rotateYXZ(0.4f, 0.1f, 0.2f * n));
        acc = h(acc, new Quaternionf().rotationAxis(0.7f + n, 0.0f, 1.0f, 0.0f));
        acc = h(acc, new Quaternionf().fromAxisAngleDeg(0.6f, 0.8f, 0.0f, 30.0f * n));
        acc = h(acc, new Quaternionf(new AxisAngle4f(1.2f, 0.0f, 0.0f, 1.0f)));
        Quaternionf r = new Quaternionf().rotateZ(1.3f).rotateX(0.2f + n);
        acc = h(acc, new Quaternionf(q).mul(r));
        acc = h(acc, new Quaternionf(q).premul(r));
        acc = h(acc, new Quaternionf(q).conjugate());
        acc = h(acc, new Quaternionf(q).invert());
        acc = h(acc, new Quaternionf(q).slerp(r, 0.35f));
        acc = h(acc, new Quaternionf(q).nlerp(r, 0.65f));
        acc = h(acc, new Quaternionf(q).scale(2.0f).normalize());
        acc = h(acc, q.transform(new Vector3f(1, 2, 3)));
        acc = h(acc, q.transformInverse(new Vector3f(-1, 0.5f, n)));
        acc = h(acc, q.getEulerAnglesXYZ(new Vector3f()));
        acc = h(acc, q.getEulerAnglesYXZ(new Vector3f()));
        acc = h(acc, new Quaternionf().lookAlong(new Vector3f(0.3f, -0.2f, 1), new Vector3f(0, 1, 0)));
        acc = h(acc, new Quaternionf().rotationTo(new Vector3f(1, 0, 0), new Vector3f(0.2f, 0.9f, n)));
        acc = h(acc, q.angle());
        acc = h(acc, new Quaternionf().setFromNormalized(new Matrix4f().rotateY(0.9f).rotateX(0.1f * n)));
        acc = h(acc, q.get(new Matrix4f()));
        acc = h(acc, q.get(new Matrix3f()));
        acc = h(acc, new Vector3f(2, n, -1).rotate(q));
        return acc;
    }

    public static long matrices(int n) {
        long acc = 17;
        Matrix4f m = new Matrix4f();
        acc = h(acc, m);
        m.translate(1.5f, -2.0f, n);
        acc = h(acc, m);
        m.rotateY(0.4f * n).rotateX(0.3f).rotateZ(-1.1f);
        acc = h(acc, m);
        m.scale(2.0f, 0.5f, 1.25f);
        acc = h(acc, m);
        m.rotate(new Quaternionf().rotateXYZ(0.1f, 0.2f, 0.3f));
        acc = h(acc, m);
        m.rotate(0.7f, new Vector3f(0.0f, 0.6f, 0.8f));
        acc = h(acc, m);
        Matrix4f other = new Matrix4f().rotationXYZ(0.5f, n * 0.25f, 0.75f).setTranslation(3, 4, 5);
        acc = h(acc, new Matrix4f(m).mul(other));
        acc = h(acc, new Matrix4f(m).mulLocal(other));
        acc = h(acc, m.mul(other, new Matrix4f()).transpose());
        acc = h(acc, new Matrix4f(m).translateLocal(1, 2, 3).scaleLocal(2.0f));
        acc = h(acc, new Matrix4f(m).rotateAround(new Quaternionf().rotateY(0.5f), 1, 2, 3));
        acc = h(acc, new Matrix4f().translation(1, n, 3).scale(4.0f));
        acc = h(acc, new Matrix4f().scaling(1.5f).translate(new Vector3f(1, 2, 3)));
        acc = h(acc, new Matrix4f().rotation(new Quaternionf().rotateX(0.25f + n)));
        acc = h(acc, new Matrix4f().rotationY(1.0f).mul(new Matrix4f().translation(1, 2, 3)));
        acc = h(acc, m.determinant());
        acc = h(acc, m.transformPosition(new Vector3f(1, 2, 3)));
        acc = h(acc, m.transformDirection(new Vector3f(0, 1, n)));
        acc = h(acc, m.transform(new Vector4f(1, 2, 3, 1)));
        acc = h(acc, m.transformProject(new Vector3f(0.5f, 0.25f, 1)));
        acc = h(acc, m.getTranslation(new Vector3f()));
        acc = h(acc, m.getScale(new Vector3f()));
        acc = h(acc, m.getNormalizedRotation(new Quaternionf()));
        acc = h(acc, m.getUnnormalizedRotation(new Quaternionf()));
        acc = h(acc, m.normal(new Matrix3f()));
        acc = h(acc, m.normal(new Matrix4f()));
        acc = h(acc, m.get3x3(new Matrix3f()));
        acc = h(acc, m.m00() + m.m11() * 2 + m.m22() * 3 + m.m30() * 5 + m.m31() * 7 + m.m32() * 11 + m.m33());
        m.m00(2.5f).m13(0.125f).m32(n);
        acc = h(acc, m);
        acc = h(acc, m.getRow(1, new Vector4f()));
        acc = h(acc, m.getColumn(2, new Vector4f()));
        acc = acc * 31 + (m.equals(new Matrix4f(m)) ? 1 : 0) + (m.equals(other) ? 2 : 0) + m.hashCode();
        acc = acc * 31 + (m.isAffine() ? 1 : 0) + (m.equals(other, 0.5f) ? 4 : 0);
        return acc;
    }

    /// Each kind of matrix JOML keeps shortcuts for, multiplied with each other.
    public static long properties(int n) {
        long acc = 19;
        Matrix4f[] kinds = {
            new Matrix4f(),
            new Matrix4f().translation(1, 2, n),
            new Matrix4f().rotationY(0.5f + n),
            new Matrix4f().translation(3, 2, 1).rotateX(0.25f).scale(2, 3, 4),
            new Matrix4f().perspective(1.2f, 1.5f, 0.1f, 100.0f),
            new Matrix4f().ortho(-2, 2, -1, 1, 0.5f, 50.0f),
            new Matrix4f().set(new float[] {1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, n + 16.5f}),
        };
        for (Matrix4f a : kinds) {
            acc = h(acc, a);
            for (Matrix4f b : kinds) {
                acc = h(acc, a.mul(b, new Matrix4f()));
            }
            acc = h(acc, new Matrix4f(a).translate(0.5f, 0.25f, 2).rotateZ(0.3f).scale(1.5f));
            acc = h(acc, new Matrix4f(a).rotate(0.4f, 1, 0, 0).rotateY(0.1f));
            acc = h(acc, a.transformPosition(new Vector3f(1, 2, 3)));
            acc = h(acc, new Matrix4f(a).transpose());
            acc = h(acc, new Matrix4f(a).mulAffine(kinds[3]));
            acc = h(acc, new Matrix4f(a).assume(0).mul(kinds[1]));
        }
        acc = h(acc, new Matrix4f().identity().determineProperties());
        acc = h(acc, kinds[6].determineProperties());
        acc = h(acc, new Matrix4f().zero());
        return acc;
    }

    public static long inverses(int n) {
        long acc = 23;
        Matrix4f affine = new Matrix4f().translate(1, n, 3).rotateXYZ(0.3f, 0.5f, 0.7f).scale(2, 0.5f, 1.5f);
        acc = h(acc, affine.invert(new Matrix4f()));
        acc = h(acc, affine.invertAffine(new Matrix4f()));
        Matrix4f rigid = new Matrix4f().rotateY(0.6f * n).translate(4, 5, 6);
        acc = h(acc, new Matrix4f(rigid).invert());
        Matrix4f general = new Matrix4f().set(new float[] {2, 0.5f, 1, 0.25f, -1, 3, n, 0.5f, 0.75f, 2, -4, 1, 0.5f, 1, 2, 3});
        acc = h(acc, general.invert(new Matrix4f()));
        acc = h(acc, general.determinant());
        Matrix4f projection = new Matrix4f().perspective(0.9f, 1.7f, 0.05f, 256.0f);
        acc = h(acc, projection.invert(new Matrix4f()));
        acc = h(acc, projection.invertPerspective(new Matrix4f()));
        acc = h(acc, new Matrix4f().translation(7, n, 9).invert());
        acc = h(acc, new Matrix4f().invert());
        acc = h(acc, new Matrix4f(affine).invert().mul(affine));
        acc = h(acc, new Matrix4f(affine).transpose3x3());
        acc = h(acc, affine.unproject(0.5f, 0.25f, 0.75f, new int[] {0, 0, 640, 480}, new Vector3f()));
        acc = h(acc, affine.project(1, 2, 3, new int[] {0, 0, 640, 480}, new Vector3f()));
        return acc;
    }

    public static long projections(int n) {
        long acc = 29;
        acc = h(acc, new Matrix4f().perspective(1.0f + 0.1f * n, 16.0f / 9.0f, 0.05f, 1000.0f));
        acc = h(acc, new Matrix4f().setPerspective(0.8f, 1.25f, 0.1f, 64.0f * n, true));
        acc = h(acc, new Matrix4f().ortho(0, 854, 480, 0, 1000, 3000));
        acc = h(acc, new Matrix4f().setOrtho(0, 427, 240, 0, 1000, 21000));
        acc = h(acc, new Matrix4f().ortho2D(0, 320, 0, 240));
        acc = h(acc, new Matrix4f().frustum(-1, 1, -0.5f, 0.5f, 0.1f, 50 + n));
        acc = h(acc, new Matrix4f().lookAt(1, 2, 3, 0, n, 0, 0, 1, 0));
        acc = h(acc, new Matrix4f().setLookAt(new Vector3f(4, 5, 6), new Vector3f(1, 0, n), new Vector3f(0, 1, 0)));
        acc = h(acc, new Matrix4f().lookAlong(0.2f, -0.3f, 1, 0, 1, 0));
        acc = h(acc, new Matrix4f().perspective(1.1f, 1.3f, 0.1f, 90).lookAt(3, 3, 3, 0, 0, 0, 0, 1, 0).translate(1, 0, n));
        acc = h(acc, new Matrix4f().billboardSpherical(new Vector3f(1, 2, 3), new Vector3f(4, n, 6)));
        acc = h(acc, new Matrix4f().reflection(0, 1, 0, 2));
        acc = h(acc, new Matrix4f().shadow(new Vector4f(1, 5, 2, 0), 0, 1, 0, 0));
        Matrix4f pv = new Matrix4f().perspective(1.0f, 1.5f, 0.1f, 100).rotateY(0.4f * n);
        acc = h(acc, pv.frustumPlane(Matrix4f.PLANE_NX, new Vector4f()));
        acc = h(acc, pv.perspectiveNear() + pv.perspectiveFar() * 3 + pv.perspectiveFov() * 5);
        acc = h(acc, pv.positiveZ(new Vector3f()));
        acc = h(acc, pv.originAffine(new Vector3f()));
        return acc;
    }

    public static long matrix3(int n) {
        long acc = 31;
        Matrix3f m = new Matrix3f();
        acc = h(acc, m);
        m.rotateY(0.3f * n).rotateX(0.7f).scale(2, 3, 0.5f);
        acc = h(acc, m);
        acc = h(acc, new Matrix3f(m).mul(new Matrix3f().rotationZ(1.1f)));
        acc = h(acc, new Matrix3f(m).invert());
        acc = h(acc, new Matrix3f(m).transpose());
        acc = h(acc, m.determinant());
        acc = h(acc, m.transform(new Vector3f(1, n, 3)));
        acc = h(acc, new Matrix3f().rotation(new Quaternionf().rotateXYZ(0.1f, 0.2f, 0.3f * n)));
        acc = h(acc, new Matrix3f(new Matrix4f().rotateZ(0.9f).translate(1, 2, 3)));
        acc = h(acc, new Matrix4f(m));
        acc = h(acc, m.getNormalizedRotation(new Quaternionf()));
        acc = h(acc, new Matrix3f(m).normal());
        acc = h(acc, m.m00() + 2 * m.m12() + 3 * m.m21());
        m.m01(4.5f).m22(n);
        acc = h(acc, m);
        acc = h(acc, m.getRow(2, new Vector3f()));
        Matrix3d d = new Matrix3d().rotateXYZ(0.2, 0.4, 0.6 * n).scale(1.5);
        double[] a = d.get(new double[9]);
        for (double x : a) acc = h(acc, x);
        acc = h(acc, d.transform(new Vector3d(1, 2, 3)));
        Matrix4x3f tall = new Matrix4x3f().translate(1, 2, n).rotateY(0.5f).scale(2);
        acc = h(acc, tall.transformPosition(new Vector3f(1, 1, 1)));
        acc = h(acc, tall.get(new Matrix4f()));
        return acc;
    }

    public static long stack(int n) {
        long acc = 37;
        Matrix4fStack s = new Matrix4fStack(8);
        s.translate(1, 2, n);
        s.pushMatrix();
        s.rotateY(0.5f).scale(2);
        acc = h(acc, s);
        s.pushMatrix();
        s.identity().translate(9, 9, 9);
        acc = h(acc, s);
        s.popMatrix();
        acc = h(acc, s);
        s.popMatrix();
        acc = h(acc, s);
        s.clear();
        acc = h(acc, s);
        return acc;
    }

    public static long arrays(int n) {
        long acc = 41;
        float[] sixteen = new float[20];
        new Matrix4f().translate(1, 2, 3).rotateX(0.5f * n).get(sixteen, 2);
        for (float f : sixteen) acc = h(acc, f);
        acc = h(acc, new Matrix4f().set(sixteen, 1));
        float[] nine = new Matrix3f().rotateZ(0.75f).get(new float[9]);
        acc = h(acc, new Matrix3f().set(nine));
        acc = h(acc, new Matrix4f().set(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, n, 5, 6, 1));
        acc = h(acc, new Matrix4f().setColumn(3, new Vector4f(1, 2, 3, 1)).setRow(0, new Vector4f(2, 0, 0, n)));
        acc = h(acc, new Matrix4f().set3x3(new Matrix3f().scale(2)));
        acc = h(acc, new Matrix4f().set(new Matrix4d().translate(1, 2, 3).rotateY(0.25)));
        acc = h(acc, new Matrix4d().set(new Matrix4f().scale(3).translate(1, n, 1)));
        Vector3f v = new Vector3f(new float[] {1, 2, 3});
        acc = h(acc, v.get(1) + v.get(2) * 3);
        acc = h(acc, new Vector3f().setComponent(1, 7.5f));
        return acc;
    }

    public static long matrix4d(int n) {
        long acc = 43;
        Matrix4d m = new Matrix4d().translate(1.5, -2, n).rotateXYZ(0.1, 0.2 * n, 0.3).scale(2, 0.5, 1.25);
        acc = h(acc, m);
        acc = h(acc, m.invert(new Matrix4d()));
        acc = h(acc, new Matrix4d(m).mul(new Matrix4d().rotationY(0.7).setTranslation(1, 2, 3)));
        acc = h(acc, m.transformPosition(new Vector3d(1, 2, 3)));
        acc = h(acc, m.determinant());
        acc = h(acc, new Matrix4d().perspective(1.0, 1.5, 0.1, 100.0).lookAt(1, 2, 3, 0, 0, n, 0, 1, 0));
        acc = h(acc, m.getNormalizedRotation(new Quaterniond()));
        acc = h(acc, m.getTranslation(new Vector3d()));
        return acc;
    }

    /// What a new object of each kind holds before anything is done to it.
    public static long identities(int n) {
        long acc = 47;
        acc = h(acc, new Matrix4f());
        acc = h(acc, new Matrix3f());
        acc = h(acc, new Matrix4d());
        acc = h(acc, new Quaternionf());
        acc = h(acc, new Quaterniond());
        acc = h(acc, new Vector3f());
        acc = h(acc, new Vector4f());
        acc = h(acc, new Matrix4fStack(2));
        return acc;
    }
}
"#;

fn has(tool: &str) -> bool {
    Command::new(tool)
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn translated_joml_matches_the_jvm() {
    if !has("javac") || !has("java") {
        eprintln!("no JDK: skipped");
        return;
    }
    let installed = horizon_mod_translator::find_game_jars().unwrap_or_default();
    let Some(joml) = installed.iter().find(|j| {
        j.file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("joml-"))
    }) else {
        eprintln!("no NeoForge install with JOML: skipped");
        return;
    };
    let root = std::env::temp_dir().join(format!("mod-translator-joml-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let classes = root.join("classes");
    fs::create_dir_all(&classes).unwrap();
    fs::write(root.join("JomlFixture.java"), FIXTURE).unwrap();

    // The JVM's answers.
    let mut runner = String::from("public class Runner { public static void main(String[] a) {\n");
    for (name, inputs) in CASES {
        for input in *inputs {
            runner.push_str(&format!(
                "System.out.println(\"{name}({input}) = \" + JomlFixture.{name}({input}));\n"
            ));
        }
    }
    runner.push_str("}}\n");
    fs::write(root.join("Runner.java"), runner).unwrap();
    let javac = Command::new("javac")
        .arg("-cp")
        .arg(joml)
        .args(["-d", classes.to_str().unwrap()])
        .arg(root.join("JomlFixture.java"))
        .arg(root.join("Runner.java"))
        .status()
        .unwrap();
    assert!(javac.success());
    let path = std::env::join_paths([classes.as_path(), joml.as_path()]).unwrap();
    let java = Command::new("java")
        .arg("-cp")
        .arg(&path)
        .arg("Runner")
        .output()
        .unwrap();
    assert!(
        java.status.success(),
        "{}",
        String::from_utf8_lossy(&java.stderr)
    );
    let expected = String::from_utf8(java.stdout).unwrap();

    // The mod's jar: the fixture alone. JOML is the install's, as a bundle takes it.
    let packed = root.join("packed");
    fs::create_dir_all(&packed).unwrap();
    for e in fs::read_dir(&classes).unwrap().flatten() {
        if e.file_name().to_string_lossy().starts_with("JomlFixture") {
            fs::copy(e.path(), packed.join(e.file_name())).unwrap();
        }
    }
    let jar = root.join("fixture.jar");
    assert!(
        Command::new("jar")
            .arg("cf")
            .arg(&jar)
            .arg("-C")
            .arg(&packed)
            .arg(".")
            .status()
            .unwrap()
            .success()
    );
    let parsed = horizon_mod_translator::program_with(&[&jar], &[], &[joml.as_path()])
        .unwrap()
        .0;
    assert!(
        parsed.iter().any(|c| c.name == "org/joml/Matrix4f"),
        "JOML's classes are part of the program"
    );
    let mut program = horizon_mod_translator::codegen::Program::new(&parsed);
    let module = program.emit();

    // The generated crate.
    let krate = root.join("gen");
    fs::create_dir_all(krate.join("src")).unwrap();
    let compat = Path::new(env!("CARGO_MANIFEST_DIR")).join("../horizon-compat");
    fs::write(
        krate.join("Cargo.toml"),
        format!(
            "[package]\nname = \"gen\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[workspace]\n[dependencies]\nhorizon-compat = {{ path = {:?} }}\n",
            compat.canonicalize().unwrap()
        ),
    )
    .unwrap();
    let mut main = String::from(
        "mod fixture;\nuse fixture::*;\nuse horizon_compat::{Rt, V};\nfn main() {\n    horizon_compat::install();\n    let mut rt = Rt::new();\n    init(&mut rt);\n",
    );
    for (name, inputs) in CASES {
        let f = horizon_mod_translator::codegen::fn_name("JomlFixture", name, "(I)J");
        for input in *inputs {
            main.push_str(&format!("    let r = rt.entered(|rt| {f}(rt, &[V::I({input})]), |r| *r); println!(\"{name}({input}) = {{}}\", r.j());\n"));
            main.push_str(&format!("    if let Some(t) = rt.thrown.take() {{ eprintln!(\"{name}({input}) threw {{}}\", rt.class_of(t)); }}\n"));
        }
    }
    // A matrix the runtime's own code made is one of JOML's: its methods are JOML's code, and
    // what the runtime writes into it leaves nothing assumed of the numbers.
    main.push_str(
        "    let made = horizon_compat::render::new_mat4(&mut rt, [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 4.0, 5.0, 6.0, 1.0]);\n    \
         let key = \"org/joml/Matrix4f.m31.()F\";\n    \
         let got = horizon_compat::linked::vcall(&mut rt, key, \"m31.()F\", &[made]);\n    \
         println!(\"native m31 = {}\", got.f());\n",
    );
    main.push_str("    let mut unmapped: Vec<_> = rt.unmapped.keys().collect();\n    unmapped.sort();\n    for k in unmapped { eprintln!(\"unmapped {k}\"); }\n");
    main.push_str("}\n");
    fs::write(krate.join("src/main.rs"), main).unwrap();
    fs::write(krate.join("src/fixture.rs"), &module).unwrap();
    let out = Command::new(env!("CARGO"))
        .args(["run", "-q", "--release", "--manifest-path"])
        .arg(krate.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", root.join("target"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "generated crate failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let actual = String::from_utf8(out.stdout).unwrap();
    let notes = String::from_utf8_lossy(&out.stderr);
    let mut wrong = Vec::new();
    for (e, a) in expected.lines().zip(actual.lines()) {
        if a != e {
            wrong.push(format!("{a} (the JVM: {e})"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}\n{notes}");
    assert_eq!(
        expected.lines().count(),
        CASES.iter().map(|c| c.1.len()).sum::<usize>(),
        "{expected}"
    );
    assert_eq!(actual.lines().last(), Some("native m31 = 5"), "{notes}");
    assert!(!notes.contains("unmapped"), "{notes}");
    assert!(program.unsupported.is_empty(), "{:?}", program.unsupported);
    // `HZ_KEEP` leaves the generated crate where it is, for a look at it.
    if std::env::var_os("HZ_KEEP").is_some() {
        eprintln!("kept: {}", root.display());
    } else {
        fs::remove_dir_all(&root).unwrap();
    }
}
