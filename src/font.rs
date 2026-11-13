use std::sync::{LazyLock, Mutex};

use fontique::Blob;
use mlua::{Lua, Result, Table, UserData, UserDataFields};
use parley::FontContext;

use crate::lua;

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

pub fn install(lua: &Lua, env: &Table) -> crate::Result<()> {
    env.set(
        "font",
        lua.create_function(|lua, path: String| -> Result<Font> {
            let module_dir = lua::module_dir(lua);
            let path = module_dir.join(&path);
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
    use mlua::{AnyUserData, Lua, Table};

    use super::{Font, install};

    fn installed() -> (Lua, Table) {
        let lua = Lua::new();
        let env = lua.create_table().unwrap();
        install(&lua, &env).unwrap();
        (lua, env)
    }

    fn call_font(
        lua: &Lua,
        env: &Table,
        source: &str,
        name: impl Into<String>,
    ) -> mlua::Result<AnyUserData> {
        lua.load(source)
            .set_name(name)
            .set_environment(env.clone())
            .call(())
    }

    #[test]
    fn the_op_registers_the_vendored_face() {
        let (lua, env) = installed();
        let face = call_font(
            &lua,
            &env,
            "return font('NotoSans-Regular.ttf')",
            "fonts/face.lua",
        )
        .unwrap();
        let font = face.borrow::<Font>().unwrap();
        assert_eq!(font.family, "Noto Sans");
    }

    #[test]
    fn the_op_rejects_a_missing_file() {
        let (lua, env) = installed();
        let error = call_font(&lua, &env, "font('missing.ttf')", "fonts/face.lua").unwrap_err();
        assert!(error.to_string().contains("failed to load"));
    }

    #[test]
    fn the_op_rejects_bytes_that_are_not_a_font() {
        let dir = std::env::temp_dir().join(format!("pikke-font-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("not-a-font.ttf");
        std::fs::write(&file, b"this is not a font file").unwrap();

        let (lua, env) = installed();
        let error = call_font(
            &lua,
            &env,
            "font('not-a-font.ttf')",
            dir.join("face.lua").display().to_string(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("not a valid font"));

        let _ = std::fs::remove_file(&file);
    }
}
