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
}

#[derive(Debug, Clone)]
pub struct Method {
    pub access: u16,
    pub name: String,
    pub desc: String,
    pub code: Option<Code>,
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
            for _ in 0..r.u2()? {
                let an = r.u2()?;
                let len = r.u4()? as usize;
                let body = r.take(len)?;
                if cf.utf8(an)? == "Code" {
                    let mut c = Reader { b: body, i: 0 };
                    let (max_stack, max_locals) = (c.u2()?, c.u2()?);
                    let n = c.u4()? as usize;
                    code = Some(Code { max_stack, max_locals, bytes: c.take(n)?.to_vec() });
                }
            }
            cf.methods.push(Method { access, name: cf.utf8(name)?, desc: cf.utf8(desc)?, code });
        }
        for _ in 0..r.u2()? {
            let an = r.u2()?;
            let len = r.u4()? as usize;
            let body = r.take(len)?;
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
}

fn skip_attributes(r: &mut Reader) -> Result<(), String> {
    for _ in 0..r.u2()? {
        r.u2()?;
        let n = r.u4()? as usize;
        r.take(n)?;
    }
    Ok(())
}
