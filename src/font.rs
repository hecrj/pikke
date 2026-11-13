use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};

use fontique::Blob;
use mlua::{Lua, Result, Table, UserData, UserDataFields};
use parley::FontContext;

use crate::{Error, lua};

pub(crate) static FONTS: LazyLock<Mutex<FontContext>> =
    LazyLock::new(|| Mutex::new(FontContext::default()));

#[derive(Clone, Debug)]
pub struct Font {
    pub family: String,
}

impl Font {
    pub fn load(path: &Path) -> crate::Result<Font> {
        let data = std::fs::read(path).map_err(|error| Error::FontReadFailed {
            path: path.to_path_buf(),
            error: Arc::new(error),
        })?;

        let mut context = FONTS.lock().unwrap();
        let registered = context.collection.register_fonts(Blob::from(data), None);
        let family_id = registered.first().map(|(family_id, _)| *family_id);
        let Some(family_id) = family_id else {
            return Err(Error::FontInvalid {
                path: path.to_path_buf(),
            });
        };

        let family = context
            .collection
            .family_name(family_id)
            .ok_or_else(|| Error::FontInvalid {
                path: path.to_path_buf(),
            })?
            .to_string();

        Ok(Font { family })
    }
}

impl UserData for Font {
    fn add_fields<F: UserDataFields<Font>>(fields: &mut F) {
        fields.add_meta_field("__name", "Font");
    }
}

pub fn install(lua: &Lua, env: &Table) -> crate::Result<()> {
    env.set(
        "font",
        lua.create_function(|lua, path: String| -> Result<Font> {
            let module_dir = lua::module_dir(lua);
            let path = module_dir.join(&path);
            Ok(Font::load(&path)?)
        })?,
    )?;
    Ok(())
}
