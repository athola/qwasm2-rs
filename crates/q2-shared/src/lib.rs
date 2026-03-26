pub mod types;
pub mod constants;
pub mod protocol;

pub use types::*;
pub use constants::*;
pub use protocol::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec3_add() {
        let a = Vec3f::new(1.0, 2.0, 3.0);
        let b = Vec3f::new(4.0, 5.0, 6.0);
        let c = a + b;
        assert_eq!(c, Vec3f::new(5.0, 7.0, 9.0));
    }
}
