//! Static world snapshot. Coordinates and spawn contact point use sixth-c1 ticks.
use alloc::vec::Vec;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cube { pub min: [i32;3], pub side: u16, pub material: u8 }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct World { pub cubes: Vec<Cube>, pub spawn: [i32;3], pub normal: [i32;3] }
impl World {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"CSW1");
        out.extend_from_slice(&(self.cubes.len() as u32).to_le_bytes());
        for v in self.spawn.iter().chain(self.normal.iter()) { out.extend_from_slice(&v.to_le_bytes()); }
        for c in &self.cubes {
            for v in c.min { out.extend_from_slice(&v.to_le_bytes()); }
            out.extend_from_slice(&c.side.to_le_bytes());
            out.extend_from_slice(&[c.material,0]);
        }
        out
    }
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len()<32 || &bytes[..4]!=b"CSW1" { return None; }
        let int = |offset| i32::from_le_bytes(bytes[offset..offset+4].try_into().unwrap());
        let count = int(4);
        if !(1..=16384).contains(&count) || bytes.len()!=32+count as usize*16 { return None; }
        let spawn = [int(8),int(12),int(16)];
        let normal = [int(20),int(24),int(28)];
        if normal.iter().any(|v| !(-1..=1).contains(v)) || normal.iter().map(|v|v.abs()).sum::<i32>()!=1
            || spawn.iter().any(|v| !(-1536..=1536).contains(v)) { return None; }
        let mut cubes = Vec::with_capacity(count as usize);
        for offset in (32..bytes.len()).step_by(16) {
            let min = [int(offset),int(offset+4),int(offset+8)];
            let side = u16::from_le_bytes([bytes[offset+12],bytes[offset+13]]);
            let material = bytes[offset+14];
            if side==0 || side>384 || material>=6 || bytes[offset+15]!=0
                || min.iter().any(|&v| v < -1536 || v > 1536-side as i32) { return None; }
            cubes.push(Cube {min,side,material});
        }
        let axis = normal.iter().position(|v|*v!=0)?;
        if !cubes.iter().any(|c| spawn[axis]==c.min[axis]+if normal[axis]>0 {c.side as i32} else {0}
            && (0..3).all(|a| a==axis || (spawn[a]>=c.min[a] && spawn[a]<=c.min[a]+c.side as i32))) { return None; }
        let outside: [i32;3] = core::array::from_fn(|a|spawn[a]+normal[a]);
        if cubes.iter().any(|c| (0..3).all(|a|outside[a]>c.min[a] && outside[a]<c.min[a]+c.side as i32)) { return None; }
        Some(Self { cubes, spawn, normal })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_roundtrip_and_validation() {
        let world=World {cubes:alloc::vec![Cube {min:[-192;3],side:384,material:0}],spawn:[0,192,0],normal:[0,1,0]};
        let bytes=world.encode();
        assert_eq!(World::parse(&bytes),Some(world));
        for end in 0..bytes.len() { assert!(World::parse(&bytes[..end]).is_none()); }
        let mut bad=bytes.clone(); bad[44..46].copy_from_slice(&0u16.to_le_bytes()); assert!(World::parse(&bad).is_none());
        let mut bad=bytes.clone(); bad[24]=2; assert!(World::parse(&bad).is_none());
        let mut bad=bytes; bad[12..16].copy_from_slice(&0i32.to_le_bytes()); assert!(World::parse(&bad).is_none());
    }
}
