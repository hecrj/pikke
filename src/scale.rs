use std::ops;

use vello_cpu::kurbo::Point;

use crate::Result;

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Scale(f32);

impl Scale {
    pub fn new(scale: f32) -> Self {
        Self(scale)
    }

    pub fn to_physical(&self, logical: f32) -> Result<u16> {
        Ok(u16::try_from((logical * self.0).round() as u32)?)
    }

    pub fn to_physical_signed(&self, logical: f32) -> Result<i32> {
        Ok(i32::try_from((logical * self.0).round() as i64)?)
    }
}

impl Default for Scale {
    fn default() -> Self {
        Self(1.0)
    }
}

impl From<f32> for Scale {
    fn from(scale: f32) -> Self {
        Self(scale)
    }
}

impl From<Scale> for f32 {
    fn from(scale: Scale) -> Self {
        scale.0
    }
}

impl From<Scale> for f64 {
    fn from(scale: Scale) -> Self {
        f64::from(scale.0)
    }
}

impl ops::Mul<Scale> for f32 {
    type Output = f32;

    fn mul(self, rhs: Scale) -> Self::Output {
        self * rhs.0
    }
}

impl ops::Mul<Scale> for Point {
    type Output = Point;

    fn mul(self, rhs: Scale) -> Self::Output {
        Point::new(self.x * rhs.0 as f64, self.y * rhs.0 as f64)
    }
}

impl ops::Div<Scale> for f32 {
    type Output = f32;

    fn div(self, rhs: Scale) -> Self::Output {
        self / rhs.0
    }
}
