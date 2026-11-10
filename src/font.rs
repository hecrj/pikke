use std::path::Path;
use std::sync::{LazyLock, Mutex};

use fontique::Blob;
use mlua::{Lua, Result, Table, UserData, UserDataFields};
use parley::FontContext;

pub(crate) static FONTS: LazyLock<Mutex<FontContext>> =
    LazyLock::new(|| Mutex::new(FontContext::default()));

#[derive(Clone, Debug)]
pub struct Font {
    pub family: String,
}

impl UserData for Font {
    fn add_fields<F: UserDataFields<Font>>(fields: &mut F) {
        fields.add_meta_field("__name", "Font");
    }
}

pub fn install(lua: &Lua, env: &Table, script_dir: &Path) -> crate::Result<()> {
    let script_dir = script_dir.to_path_buf();
    env.set(
        "font",
        lua.create_function(move |_lua, path: String| -> Result<Font> {
            let path = script_dir.join(&path);
            let data = std::fs::read(&path).map_err(|error| {
                mlua::Error::RuntimeError(format!("failed to load {}: {error}", path.display()))
            })?;
            let mut context = FONTS.lock().unwrap();
            let registered = context.collection.register_fonts(Blob::from(data), None);
            let family_id = registered.first().map(|(family_id, _)| *family_id);
            let Some(family_id) = family_id else {
                return Err(mlua::Error::RuntimeError(format!(
                    "{} is not a valid font",
                    path.display()
                )));
            };
            let family = context
                .collection
                .family_name(family_id)
                .ok_or_else(|| {
                    mlua::Error::RuntimeError(format!("{} is not a valid font", path.display()))
                })?
                .to_string();
            Ok(Font { family })
        })?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use mlua::{AnyUserData, Function, Lua, Table};

    use super::{Font, install};

    fn installed(script_dir: &Path) -> (Lua, Table, Function) {
        let lua = Lua::new();
        let env = lua.create_table().unwrap();
        install(&lua, &env, script_dir).unwrap();
        let font: Function = env.get("font").unwrap();
        (lua, env, font)
    }

    fn fonts_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fonts")
    }

    #[test]
    fn the_op_registers_the_vendored_face() {
        let (_lua, _env, font) = installed(&fonts_dir());
        let userdata: AnyUserData = font.call("NotoSans-Regular.ttf").unwrap();
        let font = userdata.borrow::<Font>().unwrap();
        assert_eq!(font.family, "Noto Sans");
    }

    #[test]
    fn the_op_rejects_a_missing_file() {
        let (_lua, _env, font) = installed(&fonts_dir());
        let error = font.call::<AnyUserData>("missing.ttf").unwrap_err();
        assert!(error.to_string().contains("failed to load"));
    }

    #[test]
    fn the_op_rejects_bytes_that_are_not_a_font() {
        let dir = std::env::temp_dir().join(format!("pikke-font-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("not-a-font.ttf");
        std::fs::write(&file, b"this is not a font file").unwrap();

        let (_lua, _env, font) = installed(&dir);
        let error = font.call::<AnyUserData>("not-a-font.ttf").unwrap_err();
        assert!(error.to_string().contains("not a valid font"));

        let _ = std::fs::remove_file(&file);
    }
}
