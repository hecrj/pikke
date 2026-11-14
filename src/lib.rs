mod color;
mod engine;
mod error;
mod font;
mod image;
mod lua;
mod paint;
mod path;
mod point;
mod rectangle;
mod scale;
mod stream;
mod text;
mod video;
mod viewport;

pub mod gradient;

pub use color::Color;
pub use engine::Engine;
pub use error::Error;
pub use gradient::Gradient;
pub use image::Image;
pub use paint::Paint;
pub use path::Path;
pub use point::Point;
pub use rectangle::Rectangle;
pub use scale::Scale;
pub use text::Text;
pub use video::Video;
pub use viewport::Viewport;

pub type Result<T> = std::result::Result<T, Error>;
