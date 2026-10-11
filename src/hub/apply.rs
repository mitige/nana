use super::*;

/// the settings a box writes when you act on an item.
pub fn apply(root: &Path, action: &Action) -> Result<String, String> {
    let mut s = settings::Settings::load(root);
    match action {
        Action::SetModel(m) => {
            s.model = Some(m.clone());
            s.provider = Some(crate::provider::detect(m).id.to_string());
            s.save_project(root).map_err(|e| e.to_string())?;
            Ok(format!("model is now {m}"))
        }
        Action::SetPersona(id) => {
            s.persona = Some(id.clone());
            s.save_project(root).map_err(|e| e.to_string())?;
            Ok(format!("persona is now {id}"))
        }
        Action::RunSkill(name) => Ok(format!("run the skill {name}")),
        Action::ReadMemory(_, _) | Action::ReadKnowledge(_) | Action::None => Ok(String::new()),
    }
}
