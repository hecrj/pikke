use crate::{Error, Result, Scale};

#[derive(Clone, Copy, Debug, Default)]
pub struct Viewport {
    width: u16,
    height: u16,
    logical_width: f32,
    logical_height: f32,
    scale: Scale,
}

impl Viewport {
    pub fn zero(scale: impl Into<Scale>) -> Self {
        Self {
            width: 0,
            height: 0,
            logical_width: 0.0,
            logical_height: 0.0,
            scale: scale.into(),
        }
    }

    pub fn from_physical_u32(width: u32, height: u32, scale: impl Into<Scale>) -> Result<Self> {
        let scale = scale.into();

        let width = u16::try_from(width)?;
        let height = u16::try_from(height)?;

        Ok(Self {
            width,
            height,
            logical_width: f32::from(width) / scale,
            logical_height: f32::from(height) / scale,
            scale,
        })
    }

    pub fn from_logical(width: f32, height: f32, scale: impl Into<Scale>) -> Result<Self> {
        let scale = scale.into();
        let physical_width = scale.to_physical(width)?;
        let physical_height = scale.to_physical(height)?;

        if physical_width == 0 || physical_height == 0 {
            return Err(Error::InvalidDimensions {
                width: physical_width,
                height: physical_height,
            });
        }

        Ok(Self {
            width: physical_width,
            height: physical_height,
            logical_width: width,
            logical_height: height,
            scale,
        })
    }

    pub fn rotated(self) -> Self {
        Self {
            width: self.height,
            height: self.width,
            logical_width: self.logical_height,
            logical_height: self.logical_width,
            scale: self.scale,
        }
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    pub fn logical_width(&self) -> f32 {
        self.logical_width
    }

    pub fn logical_height(&self) -> f32 {
        self.logical_height
    }

    pub fn scale(&self) -> Scale {
        self.scale
    }

    pub fn total_pixels(&self) -> usize {
        self.width as usize * self.height as usize
    }
}

#[cfg(test)]
mod tests {
    use super::{Scale, Viewport};

    #[test]
    fn the_transform_is_exact() -> crate::Result<()> {
        let viewport = Viewport::from_physical_u32(512, 512, Scale::new(0.5))?;

        assert_eq!(viewport.scale().to_physical(200.0)?, 100);
        assert_eq!(viewport.scale().to_physical(20.0)?, 10);
        assert_eq!(viewport.scale().to_physical(150.0)?, 75);
        assert_eq!(viewport.scale().to_physical_signed(-20.0)?, -10);
        assert_eq!(viewport.scale().to_physical_signed(-21.0)?, -11);

        Ok(())
    }
}
