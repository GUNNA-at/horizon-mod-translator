//! JVM instruction decoding (JVMS chapter 6): byte offsets, opcodes and
//! operands, including `tableswitch`, `lookupswitch` and `wide`.

#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    None,
    /// A local slot, a constant-pool index, a byte/short immediate.
    Int(i32),
    /// `iinc`: slot, delta.
    Iinc(u16, i32),
    /// Branch target, absolute offset.
    Jump(usize),
    /// `invokeinterface` / `multianewarray`: index plus a count.
    IndexCount(u16, u8),
    /// default, (match, target) pairs; absolute offsets.
    Switch(usize, Vec<(i32, usize)>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Insn {
    pub at: usize,
    pub op: u8,
    pub arg: Operand,
}

pub mod op {
    pub const ACONST_NULL: u8 = 0x01;
    pub const BIPUSH: u8 = 0x10;
    pub const SIPUSH: u8 = 0x11;
    pub const LDC: u8 = 0x12;
    pub const LDC_W: u8 = 0x13;
    pub const LDC2_W: u8 = 0x14;
    pub const GETSTATIC: u8 = 0xb2;
    pub const PUTSTATIC: u8 = 0xb3;
    pub const GETFIELD: u8 = 0xb4;
    pub const PUTFIELD: u8 = 0xb5;
    pub const INVOKEVIRTUAL: u8 = 0xb6;
    pub const INVOKESPECIAL: u8 = 0xb7;
    pub const INVOKESTATIC: u8 = 0xb8;
    pub const INVOKEINTERFACE: u8 = 0xb9;
    pub const INVOKEDYNAMIC: u8 = 0xba;
    pub const NEW: u8 = 0xbb;
    pub const CHECKCAST: u8 = 0xc0;
}

fn be16(b: &[u8], i: usize) -> Result<i32, String> {
    let s = b.get(i..i + 2).ok_or("truncated code")?;
    Ok(i16::from_be_bytes([s[0], s[1]]) as i32)
}

fn be32(b: &[u8], i: usize) -> Result<i32, String> {
    let s = b.get(i..i + 4).ok_or("truncated code")?;
    Ok(i32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

fn u8_at(b: &[u8], i: usize) -> Result<u8, String> {
    b.get(i).copied().ok_or_else(|| "truncated code".into())
}

/// Decodes a method body.
pub fn decode(b: &[u8]) -> Result<Vec<Insn>, String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let at = i;
        let op = b[i];
        i += 1;
        let jump = |off: i32| (at as i64 + off as i64) as usize;
        let arg = match op {
            // local index (u1) loads/stores and ret, newarray (atype), bipush (s1), ldc (u1)
            0x15..=0x19 | 0x36..=0x3a | 0xa9 | 0xbc => {
                i += 1;
                Operand::Int(u8_at(b, i - 1)? as i32)
            }
            0x10 => {
                i += 1;
                Operand::Int(u8_at(b, i - 1)? as i8 as i32)
            }
            0x12 => {
                i += 1;
                Operand::Int(u8_at(b, i - 1)? as i32)
            }
            0x11 => {
                i += 2;
                Operand::Int(be16(b, i - 2)?)
            }
            // u2 pool index
            0x13 | 0x14 | 0xb2..=0xb8 | 0xbb | 0xbd | 0xc0 | 0xc1 => {
                i += 2;
                Operand::Int(be16(b, i - 2)? as u16 as i32)
            }
            0xb9 => {
                i += 4;
                Operand::IndexCount(be16(b, i - 4)? as u16, u8_at(b, i - 2)?)
            }
            0xba => {
                i += 4;
                Operand::Int(be16(b, i - 4)? as u16 as i32)
            }
            0xc5 => {
                i += 3;
                Operand::IndexCount(be16(b, i - 3)? as u16, u8_at(b, i - 1)?)
            }
            0x84 => {
                i += 2;
                Operand::Iinc(u8_at(b, i - 2)? as u16, u8_at(b, i - 1)? as i8 as i32)
            }
            // branches, s2 offset
            0x99..=0xa8 | 0xc6 | 0xc7 => {
                i += 2;
                Operand::Jump(jump(be16(b, i - 2)?))
            }
            0xc8 | 0xc9 => {
                i += 4;
                Operand::Jump(jump(be32(b, i - 4)?))
            }
            0xaa => {
                i = (i + 3) & !3;
                let default = jump(be32(b, i)?);
                let (lo, hi) = (be32(b, i + 4)?, be32(b, i + 8)?);
                i += 12;
                let mut pairs = Vec::new();
                for k in lo..=hi {
                    pairs.push((k, jump(be32(b, i)?)));
                    i += 4;
                }
                Operand::Switch(default, pairs)
            }
            0xab => {
                i = (i + 3) & !3;
                let default = jump(be32(b, i)?);
                let n = be32(b, i + 4)?;
                i += 8;
                let mut pairs = Vec::new();
                for _ in 0..n {
                    pairs.push((be32(b, i)?, jump(be32(b, i + 4)?)));
                    i += 8;
                }
                Operand::Switch(default, pairs)
            }
            0xc4 => {
                let inner = u8_at(b, i)?;
                let slot = be16(b, i + 1)? as u16;
                if inner == 0x84 {
                    let d = be16(b, i + 3)?;
                    i += 5;
                    out.push(Insn { at, op: inner, arg: Operand::Iinc(slot, d) });
                } else {
                    i += 3;
                    out.push(Insn { at, op: inner, arg: Operand::Int(slot as i32) });
                }
                continue;
            }
            _ => Operand::None,
        };
        out.push(Insn { at, op, arg });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_operands_and_branch_targets() {
        // iconst_0; istore_1; iload_1; bipush -3; if_icmpge +7; iinc 1 1; goto -9; return
        let code = [0x03, 0x3c, 0x1b, 0x10, 0xfd, 0xa2, 0x00, 0x09, 0x84, 0x01, 0x01, 0xa7, 0xff, 0xf7, 0xb1];
        let insns = decode(&code).unwrap();
        let ops: Vec<u8> = insns.iter().map(|i| i.op).collect();
        assert_eq!(ops, [0x03, 0x3c, 0x1b, 0x10, 0xa2, 0x84, 0xa7, 0xb1]);
        assert_eq!(insns[3].arg, Operand::Int(-3));
        assert_eq!(insns[4].arg, Operand::Jump(14));
        assert_eq!(insns[5].arg, Operand::Iinc(1, 1));
        assert_eq!(insns[6].arg, Operand::Jump(2));
    }

    #[test]
    fn tableswitch_is_aligned() {
        // nop; tableswitch at 1, padding to 4, default +20, 0..=1 → +12, +16
        let mut code = vec![0x00, 0xaa, 0, 0];
        for v in [20i32, 0, 1, 12, 16] {
            code.extend(v.to_be_bytes());
        }
        code.push(0xb1);
        let insns = decode(&code).unwrap();
        assert_eq!(insns[1].arg, Operand::Switch(21, vec![(0, 13), (1, 17)]));
        assert_eq!(insns[2].op, 0xb1);
    }
}
