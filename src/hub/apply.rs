use super::*;

/// the settings a box writes when you act on an item.
pub fn apply(root: &Path, action: &Action) -> Result<String, String> {
    let mut s = settings::Settings::load(root);
    match action {
        Action::SetModel(m) => {
            // the model decides the endpoint here, so a provider picked
            // earlier must not outvote it: clear it before asking
            s.model = Some(m.clone());
            s.provider = None;
            s.provider = Some(crate::provider::Endpoint::resolve(&s, m).id);
            s.save_project(root).map_err(|e| e.to_string())?;
            Ok(format!("model is now {m}"))
        }
        Action::SetProvider(id) => {
            // the model stays; only the endpoint answering it changes. when the
            // endpoint serves a named model, that name is written too, so the
            // pair in settings.json reads back the way it was chosen.
            let endpoints = crate::provider::Endpoint::all(&s);
            let picked = endpoints
                .iter()
                .find(|e| &e.id == id)
                .ok_or_else(|| format!("no provider named {id}"))?;
            s.provider = Some(id.clone());
            if let Some(model) = picked.models.first() {
                s.model = Some(model.clone());
            }
            s.save_project(root).map_err(|e| e.to_string())?;
            Ok(format!("{id} answers from now on"))
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

/// Adds one custom endpoint to the project settings and makes it the one that
/// answers, from the line typed in the hub. Hands back the sentence to show.
pub fn add_provider(root: &Path, line: &str) -> Result<String, String> {
    let (id, cfg) = settings::ProviderCfg::parse(line)?;
    let mut s = settings::Settings::load(root);
    s.providers.insert(id.clone(), cfg);
    s.provider = Some(id.clone());
    s.save_project(root).map_err(|e| e.to_string())?;
    Ok(format!("{id} added — it answers from now on"))
}
