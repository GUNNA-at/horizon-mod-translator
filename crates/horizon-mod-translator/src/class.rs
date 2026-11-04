//! A full JVM class file reader (JVMS chapter 4): constant pool, fields,
//! methods with their `Code` attributes. Enough for the translator to see
//! every instruction and every member reference.

#[derive(Debug, Clone, PartialEq)]
pub enum Const {
    Unused,
    Utf8(String),
    Int(i32),
    Float(f32),
    Long(i64),
    Double(f64),
    Class(u16),
    String(u16),
    FieldRef(u16, u16),
    MethodRef(u16, u16),
    InterfaceMethodRef(u16, u16),
    NameAndType(u16, u16),
    MethodHandle(u8, u16),
    MethodType(u16),
    Dynamic(u16, u16),
    InvokeDynamic(u16, u16),
    Module(u16),
    Package(u16),
}

/// A resolved member reference: owner class, name, descriptor.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Member {
    pub owner: String,
    pub name: String,
    pub desc: String,
}

#[derive(Debug, Clone, Default)]
pub struct Code {
    pub max_stack: u16,
    pub max_locals: u16,
    pub bytes: Vec<u8>,
    /// (start, end, handler, catch class or "" for any).
    pub handlers: Vec<(u16, u16, u16, String)>,
}

#[derive(Debug, Clone)]
pub struct Method {
    pub access: u16,
    pub name: String,
    pub desc: String,
    pub code: Option<Code>,
    /// Its annotations (`@SubscribeEvent`...), the class-file-only ones (a mixin's `@Inject`) too.
    pub annotations: Vec<Annotation>,
}

/// A runtime-visible annotation: its type and element values, each as text (a constant, an enum
/// constant's name, a class descriptor; an array's items joined by `,`).
#[derive(Debug, Clone, Default)]
pub struct Annotation {
    /// The type's descriptor, as `Lnet/neoforged/bus/api/SubscribeEvent;`.
    pub ty: String,
    pub values: Vec<(String, String)>,
}

impl Annotation {
    pub fn value(&self, name: &str) -> Option<&str> {
        self.values.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct Field {
    pub access: u16,
    pub name: String,
    pub desc: String,
}

#[derive(Debug, Clone)]
pub struct ClassFile {
    pub pool: Vec<Const>,
    pub access: u16,
    pub name: String,
    pub superclass: Option<String>,
    pub interfaces: Vec<String>,
    pub fields: Vec<Field>,
    pub methods: Vec<Method>,
    /// `BootstrapMethods`: (method handle index, static argument indices).
    pub bootstrap: Vec<(u16, Vec<u16>)>,
    /// Its annotations (`@Mod`, `@EventBusSubscriber`...), the class-file-only ones (`@Mixin`) too.
    pub annotations: Vec<Annotation>,
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let s = self.b.get(self.i..self.i + n).ok_or("truncated class file")?;
        self.i += n;
        Ok(s)
    }
    fn u1(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u2(&mut self) -> Result<u16, String> {
        let s = self.take(2)?;
        Ok(u16::from_be_bytes([s[0], s[1]]))
    }
    fn u4(&mut self) -> Result<u32, String> {
        let s = self.take(4)?;
        Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
}

/// Java's "modified UTF-8": close enough to UTF-8 for names; nulls and
/// surrogate pairs are decoded leniently.
fn mutf8(b: &[u8]) -> String {
    String::from_utf8(b.to_vec()).unwrap_or_else(|_| String::from_utf8_lossy(b).into_owned())
}

impl ClassFile {
    pub fn parse(bytes: &[u8]) -> Result<ClassFile, String> {
        let mut r = Reader { b: bytes, i: 0 };
        if r.u4()? != 0xCAFE_BABE {
            return Err("not a class file".into());
        }
        r.u2()?;
        r.u2()?;
        let count = r.u2()? as usize;
        let mut pool = vec![Const::Unused];
        while pool.len() < count {
            let tag = r.u1()?;
            let c = match tag {
                1 => {
                    let n = r.u2()? as usize;
                    Const::Utf8(mutf8(r.take(n)?))
                }
                3 => Const::Int(r.u4()? as i32),
                4 => Const::Float(f32::from_bits(r.u4()?)),
                5 => Const::Long(((r.u4()? as u64) << 32 | r.u4()? as u64) as i64),
                6 => Const::Double(f64::from_bits((r.u4()? as u64) << 32 | r.u4()? as u64)),
                7 => Const::Class(r.u2()?),
                8 => Const::String(r.u2()?),
                9 => Const::FieldRef(r.u2()?, r.u2()?),
                10 => Const::MethodRef(r.u2()?, r.u2()?),
                11 => Const::InterfaceMethodRef(r.u2()?, r.u2()?),
                12 => Const::NameAndType(r.u2()?, r.u2()?),
                15 => Const::MethodHandle(r.u1()?, r.u2()?),
                16 => Const::MethodType(r.u2()?),
                17 => Const::Dynamic(r.u2()?, r.u2()?),
                18 => Const::InvokeDynamic(r.u2()?, r.u2()?),
                19 => Const::Module(r.u2()?),
                20 => Const::Package(r.u2()?),
                t => return Err(format!("bad constant tag {t}")),
            };
            let wide = matches!(c, Const::Long(_) | Const::Double(_));
            pool.push(c);
            if wide {
                pool.push(Const::Unused);
            }
        }
        let mut cf = ClassFile {
            pool,
            access: 0,
            name: String::new(),
            superclass: None,
            interfaces: Vec::new(),
            fields: Vec::new(),
            methods: Vec::new(),
            bootstrap: Vec::new(),
            annotations: Vec::new(),
        };
        cf.access = r.u2()?;
        cf.name = cf.class_name(r.u2()?)?;
        let sup = r.u2()?;
        cf.superclass = if sup == 0 { None } else { Some(cf.class_name(sup)?) };
        for _ in 0..r.u2()? {
            let i = r.u2()?;
            cf.interfaces.push(cf.class_name(i)?);
        }
        for _ in 0..r.u2()? {
            let (access, name, desc) = (r.u2()?, r.u2()?, r.u2()?);
            cf.fields.push(Field { access, name: cf.utf8(name)?, desc: cf.utf8(desc)? });
            skip_attributes(&mut r)?;
        }
        for _ in 0..r.u2()? {
            let (access, name, desc) = (r.u2()?, r.u2()?, r.u2()?);
            let mut code = None;
            let mut annotations = Vec::new();
            for _ in 0..r.u2()? {
                let an = r.u2()?;
                let len = r.u4()? as usize;
                let body = r.take(len)?;
                // Class-file-only ones too: what a mixin injects where (`@Inject`, `@At`).
                if matches!(cf.utf8(an)?.as_str(), "RuntimeVisibleAnnotations" | "RuntimeInvisibleAnnotations") {
                    let more = cf.annotations(body)?;
                    annotations.extend(more);
                }
                if cf.utf8(an)? == "Code" {
                    let mut c = Reader { b: body, i: 0 };
                    let (max_stack, max_locals) = (c.u2()?, c.u2()?);
                    let n = c.u4()? as usize;
                    let bytes = c.take(n)?.to_vec();
                    let mut handlers = Vec::new();
                    for _ in 0..c.u2()? {
                        let (start, end, handler, ty) = (c.u2()?, c.u2()?, c.u2()?, c.u2()?);
                        let class = if ty == 0 { String::new() } else { cf.class_name(ty)? };
                        handlers.push((start, end, handler, class));
                    }
                    code = Some(Code { max_stack, max_locals, bytes, handlers });
                }
            }
            cf.methods.push(Method { access, name: cf.utf8(name)?, desc: cf.utf8(desc)?, code, annotations });
        }
        for _ in 0..r.u2()? {
            let an = r.u2()?;
            let len = r.u4()? as usize;
            let body = r.take(len)?;
            // Invisible ones too: `@Mixin` (its target) is kept only in the class file.
            if matches!(cf.utf8(an)?.as_str(), "RuntimeVisibleAnnotations" | "RuntimeInvisibleAnnotations") {
                let more = cf.annotations(body)?;
                cf.annotations.extend(more);
            }
            if cf.utf8(an)? == "BootstrapMethods" {
                let mut b = Reader { b: body, i: 0 };
                for _ in 0..b.u2()? {
                    let handle = b.u2()?;
                    let n = b.u2()?;
                    let args = (0..n).map(|_| b.u2()).collect::<Result<Vec<_>, _>>()?;
                    cf.bootstrap.push((handle, args));
                }
            }
        }
        Ok(cf)
    }

    /// A `RuntimeVisibleAnnotations` attribute's annotations.
    fn annotations(&self, body: &[u8]) -> Result<Vec<Annotation>, String> {
        let mut r = Reader { b: body, i: 0 };
        (0..r.u2()?).map(|_| self.annotation(&mut r)).collect()
    }

    fn annotation(&self, r: &mut Reader) -> Result<Annotation, String> {
        let ty = self.utf8(r.u2()?)?;
        let mut values = Vec::new();
        for _ in 0..r.u2()? {
            let name = self.utf8(r.u2()?)?;
            values.push((name, self.element_value(r)?));
        }
        Ok(Annotation { ty, values })
    }

    fn element_value(&self, r: &mut Reader) -> Result<String, String> {
        Ok(match r.u1()? {
            b's' => self.utf8(r.u2()?)?,
            b'e' => {
                r.u2()?; // the enum's type
                self.utf8(r.u2()?)?
            }
            b'c' => self.utf8(r.u2()?)?,
            // A nested one with its values, `|` between them (an array's are `,`, and a
            // descriptor has `;`): `Lorg/spongepowered/asm/mixin/injection/At;(value=INVOKE|target=...)`.
            b'@' => {
                let a = self.annotation(r)?;
                let values: Vec<String> = a.values.iter().map(|(k, v)| format!("{k}={v}")).collect();
                format!("{}({})", a.ty, values.join("|"))
            }
            b'[' => (0..r.u2()?).map(|_| self.element_value(r)).collect::<Result<Vec<_>, _>>()?.join(","),
            _ => match self.pool.get(r.u2()? as usize) {
                Some(Const::Int(i)) => i.to_string(),
                Some(Const::Long(j)) => j.to_string(),
                Some(Const::Float(f)) => f.to_string(),
                Some(Const::Double(d)) => d.to_string(),
                Some(Const::Utf8(s)) => s.clone(),
                _ => String::new(),
            },
        })
    }

    /// The annotation of type `ty` (a descriptor), when the class has it.
    pub fn annotation_of(&self, ty: &str) -> Option<&Annotation> {
        self.annotations.iter().find(|a| a.ty == ty)
    }

    /// Every class a mixin targets (`@Mixin({A.class, B.class})`, `targets = {"a.B", ...}`), as
    /// internal names; empty for a class that is no mixin.
    pub fn mixin_targets(&self) -> Vec<String> {
        let Some(m) = self.annotation_of("Lorg/spongepowered/asm/mixin/Mixin;") else { return Vec::new() };
        let mut out: Vec<String> = Vec::new();
        for t in m.value("value").unwrap_or("").split(',').filter(|t| !t.is_empty()) {
            out.push(t.strip_prefix('L').and_then(|t| t.strip_suffix(';')).unwrap_or(t).to_owned());
        }
        for t in m.value("targets").unwrap_or("").split(',').filter(|t| !t.is_empty()) {
            out.push(t.replace('.', "/"));
        }
        out
    }

    /// The game class a mixin targets (`@Mixin(ShapedRecipe.class)`, or `targets = "..."`), as
    /// `net/minecraft/world/item/crafting/ShapedRecipe`; its first when it names several.
    pub fn mixin_target(&self) -> Option<String> {
        let m = self.annotation_of("Lorg/spongepowered/asm/mixin/Mixin;")?;
        let first = |v: &str| v.split(',').next().filter(|t| !t.is_empty()).map(str::to_owned);
        if let Some(t) = m.value("value").and_then(first) {
            return Some(t.strip_prefix('L').and_then(|t| t.strip_suffix(';')).unwrap_or(&t).to_owned());
        }
        m.value("targets").and_then(first).map(|t| t.replace('.', "/"))
    }

    pub fn utf8(&self, i: u16) -> Result<String, String> {
        match self.pool.get(i as usize) {
            Some(Const::Utf8(s)) => Ok(s.clone()),
            _ => Err(format!("constant {i} is not Utf8")),
        }
    }

    pub fn class_name(&self, i: u16) -> Result<String, String> {
        match self.pool.get(i as usize) {
            Some(Const::Class(n)) => self.utf8(*n),
            _ => Err(format!("constant {i} is not a class")),
        }
    }

    /// A field or method reference at pool index `i`.
    pub fn member(&self, i: u16) -> Option<Member> {
        let (c, nt) = match self.pool.get(i as usize)? {
            Const::FieldRef(c, nt) | Const::MethodRef(c, nt) | Const::InterfaceMethodRef(c, nt) => {
                (*c, *nt)
            }
            _ => return None,
        };
        let Const::NameAndType(n, d) = self.pool.get(nt as usize)? else {
            return None;
        };
        Some(Member {
            owner: self.class_name(c).ok()?,
            name: self.utf8(*n).ok()?,
            desc: self.utf8(*d).ok()?,
        })
    }

    /// The method a `MethodHandle` constant points at.
    pub fn handle(&self, i: u16) -> Option<(u8, Member)> {
        match self.pool.get(i as usize)? {
            Const::MethodHandle(kind, r) => Some((*kind, self.member(*r)?)),
            _ => None,
        }
    }

    /// An `invokedynamic` site: (bootstrap index, name, descriptor).
    pub fn indy(&self, i: u16) -> Option<(u16, String, String)> {
        let Const::InvokeDynamic(bsm, nt) = self.pool.get(i as usize)? else { return None };
        let Const::NameAndType(n, d) = self.pool.get(*nt as usize)? else { return None };
        Some((*bsm, self.utf8(*n).ok()?, self.utf8(*d).ok()?))
    }

    /// Every method reference in the pool (what the class calls).
    pub fn called(&self) -> Vec<Member> {
        (0..self.pool.len() as u16)
            .filter(|&i| {
                matches!(
                    self.pool[i as usize],
                    Const::MethodRef(..) | Const::InterfaceMethodRef(..)
                )
            })
            .filter_map(|i| self.member(i))
            .collect()
    }

    /// The methods one of this class's methods calls (its invoke instructions' targets).
    pub fn called_by(&self, m: &Method) -> Vec<Member> {
        let Some(code) = &m.code else { return Vec::new() };
        crate::insn::decode(&code.bytes)
            .unwrap_or_default()
            .iter()
            .filter(|i| matches!(i.op, 0xb6..=0xb9))
            .filter_map(|i| match i.arg {
                crate::insn::Operand::Int(idx) => self.member(idx as u16),
                crate::insn::Operand::IndexCount(idx, _) => self.member(idx),
                _ => None,
            })
            .collect()
    }
}

fn skip_attributes(r: &mut Reader) -> Result<(), String> {
    for _ in 0..r.u2()? {
        r.u2()?;
        let n = r.u4()? as usize;
        r.take(n)?;
    }
    Ok(())
}

/// A method descriptor's parameter types, as class names (`I` for an int, `[Lx;` for arrays).
pub fn param_classes(desc: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Some(params) = desc.strip_prefix('(').and_then(|d| d.split(')').next()) else { return out };
    let b = params.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let start = i;
        while i < b.len() && b[i] == b'[' {
            i += 1;
        }
        if i < b.len() && b[i] == b'L' {
            let end = params[i..].find(';').map_or(b.len(), |e| i + e);
            out.push(if start == i { params[i + 1..end].to_owned() } else { params[start..=end].to_owned() });
            i = end + 1;
        } else {
            out.push(params[start..=i.min(b.len() - 1)].to_owned());
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod param_tests {
    #[test]
    fn param_classes_of_a_mod_constructor() {
        assert_eq!(
            super::param_classes("(Lnet/neoforged/bus/api/IEventBus;Lnet/neoforged/fml/ModContainer;I[J)V"),
            ["net/neoforged/bus/api/IEventBus", "net/neoforged/fml/ModContainer", "I", "[J"]
        );
    }
}
