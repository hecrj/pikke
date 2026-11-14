use crate::{Color, Gradient};

#[derive(Debug, Clone, PartialEq)]
pub enum Paint {
    Color(Color),
    Gradient(Gradient),
}

impl From<Color> for Paint {
    fn from(color: Color) -> Self {
        Self::Color(color)
    }
}

impl From<Gradient> for Paint {
    fn from(gradient: Gradient) -> Self {
        Self::Gradient(gradient)
    }
}
