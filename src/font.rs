use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use fontique::Blob;
use mlua::{Lua, Result, Table, UserData, UserDataFields};
use parley::FontContext;

use crate::{Error, lua};

pub(crate) static FONTS: LazyLock<Mutex<FontContext>> =
    LazyLock::new(|| Mutex::new(FontContext::default()));

static FAMILIES: LazyLock<Mutex<HashMap<PathBuf, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Debug)]
pub struct Font {
    pub family: String,
}

impl Font {
    pub fn load(path: &Path) -> crate::Result<Font> {
        if let Some(family) = FAMILIES.lock().unwrap().get(path) {
            return Ok(Font {
                family: family.clone(),
            });
        }

        let data = std::fs::read(path).map_err(|error| Error::FontReadFailed {
            path: path.to_path_buf(),
            error: Arc::new(error),
        })?;

        let family = {
            let mut families = FAMILIES.lock().unwrap();

            if let Some(family) = families.get(path) {
                family.clone()
            } else {
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

                families.insert(path.to_path_buf(), family.clone());
                family
            }
        };

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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::Font;

    #[test]
    fn the_font_is_loaded_once_and_cached() {
        let dir = std::env::temp_dir().join(format!("pikke-font-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("fonts/NotoSans-Regular.ttf");
        let path = dir.join("NotoSans-Regular.ttf");
        std::fs::copy(&source, &path).unwrap();

        let first = Font::load(&path).unwrap();
        let family = first.family.clone();
        assert!(!family.is_empty());

        std::fs::remove_file(&path).unwrap();
        let second = Font::load(&path).unwrap();

        assert_eq!(second.family, family);
    }
}
