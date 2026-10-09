//! Couche IA centralisée : UN SEUL client pour tous les agents de c-man.
//!
//! Provider : Kimi K3 via l'endpoint OpenAI-compatible du projet DSH.
//! Clé API : env `OPP_API_KEY`, sinon `~/.dsh/.credentials.yaml` (jamais affichée).

use crate::config::Config;
use std::fmt;
use std::path::PathBuf;

const DEFAULT_BASE_URL: &str = "https://dashscope.aliyuncs.com/compatible-mode/v1";
const DEFAULT_MODEL: &str = "kimi-k3";

/// Fenêtre de contexte des modèles du provider : 1M tokens.
pub const CONTEXT_WINDOW_TOKENS: u32 = 1_048_576; // 1M
/// Sortie maximale des modèles du provider : 128K tokens.
pub const MAX_OUTPUT_TOKENS: u32 = 131_072; // 128K

/// Modèles servis par le provider du projet
/// (contexte 1M, sortie max 128K — cf. constantes ci-dessus).
pub const AVAILABLE_MODELS: &[&str] = &[
    "kimi-k3",
    "deepseek-v4.1-flash",
    "glm-5.3-prime",
    "qwen3.8-max-0902",
];

// ------------------------------------------------------------------ erreurs

#[derive(Debug)]
pub enum AiError {
    NoKey,
    Network(String),
    Http(u16, String),
    InvalidResponse(String),
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AiError::NoKey => write!(
                f,
                "api key not found. set opp_api_key or add it to ~/.dsh/.credentials.yaml"
            ),
            AiError::Network(e) => write!(f, "network error: {e}"),
            AiError::Http(code, body) => write!(f, "http {code}: {}", &body[..body.len().min(200)]),
            AiError::InvalidResponse(e) => write!(f, "invalid ai response: {e}"),
        }
    }
}

// ------------------------------------------------------------------ client

#[derive(Clone)]
pub struct AiClient {
    base_url: String,
    api_key: String,
    pub model: String,
}

impl AiClient {
    pub fn from_env(cfg: &Config) -> Result<Self, AiError> {
        let api_key = std::env::var("OPP_API_KEY")
            .ok()
            .filter(|k| !k.is_empty())
            .or_else(read_dsh_credentials_key)
            .ok_or(AiError::NoKey)?;
        let base_url = std::env::var("CMAN_AI_BASE")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        let model = std::env::var("CMAN_AI_MODEL")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| cfg.ai_model.clone())
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());
        Ok(Self {
            base_url,
            api_key,
            model,
        })
    }

    /// Appel chat completions (non-streaming). Bloquant — à lancer dans un thread.
    /// Budget de sortie = plafond du provider (128K). Retries automatiques :
    /// réponse vide (raisonnement a tout mangé) → budget ×4 borné à 128K ;
    /// erreur transitoire (429/5xx/réseau) → nouvelle tentative.
    pub fn chat(&self, system: &str, user: &str) -> Result<String, AiError> {
        self.chat_resilient(system, user, MAX_OUTPUT_TOKENS)
    }

    /// Appel rapide pour l'interactif (complétion) : petit budget, timeout court.
    pub fn chat_fast(&self, system: &str, user: &str) -> Result<String, AiError> {
        self.chat_with_budget(system, user, 1024)
    }

    /// Appel avec le budget de sortie maximal (plans, synthèses longues) :
    /// 128K tokens, de quoi ne jamais couper un plan en plein milieu.
    pub fn chat_long(&self, system: &str, user: &str) -> Result<String, AiError> {
        self.chat_resilient(system, user, MAX_OUTPUT_TOKENS)
    }

    /// Change le modèle utilisé par ce client.
    pub fn set_model(&mut self, model: &str) {
        self.model = model.to_string();
    }

    /// Modèle suivant dans la liste des modèles disponibles.
    pub fn next_model_name(&self) -> &'static str {
        let pos = AVAILABLE_MODELS
            .iter()
            .position(|m| *m == self.model)
            .unwrap_or(0);
        AVAILABLE_MODELS[(pos + 1) % AVAILABLE_MODELS.len()]
    }

    /// Une tentative + retries ciblés selon la cause de l'échec.
    fn chat_resilient(&self, system: &str, user: &str, max_tokens: u32) -> Result<String, AiError> {
        let mut budget = max_tokens;
        let mut last_err = None;
        for attempt in 0..3 {
            match self.chat_with_budget(system, user, budget) {
                Ok(r) => return Ok(r),
                Err(AiError::InvalidResponse(e)) if e.contains("empty response") => {
                    // le raisonnement a épuisé le budget → on quadruple,
                    // borné au plafond du provider (128K)
                    budget = budget.saturating_mul(4).min(MAX_OUTPUT_TOKENS);
                    last_err = Some(AiError::InvalidResponse(e));
                }
                Err(AiError::Http(400, _)) if budget > 8192 => {
                    // ce modèle refuse ce budget de sortie → on divise par 2
                    // et on retente avec un plafond réaliste
                    budget /= 2;
                    std::thread::sleep(std::time::Duration::from_secs(2 + attempt * 3));
                    last_err = Some(AiError::Http(400, "output budget refused".into()));
                }
                Err(e @ AiError::Http(_, _)) => {
                    // 429/5xx transitoires ; les 400 sont fréquents sur ce
                    // provider (load-balancing entre backends) → on retente aussi
                    std::thread::sleep(std::time::Duration::from_secs(2 + attempt * 3));
                    last_err = Some(e);
                }
                Err(e @ AiError::Network(_)) => {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    last_err = Some(e);
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err.unwrap_or(AiError::InvalidResponse("failed after retries".into())))
    }

    fn chat_with_budget(
        &self,
        system: &str,
        user: &str,
        max_tokens: u32,
    ) -> Result<String, AiError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user}
            ],
            "max_tokens": max_tokens,
        });
        // timeout : 90s de marge réseau + génération à ~30 tok/s, plafond 1 h
        // (un budget 128K laisse le temps aux modèles à raisonnement)
        let timeout_secs = (90 + u64::from(max_tokens) / 30).min(3600);
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(timeout_secs)))
            .build()
            .into();
        let response = agent
            .post(&url)
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .send_json(&body);
        match response {
            Ok(mut resp) => {
                let value: serde_json::Value = resp
                    .body_mut()
                    .read_json()
                    .map_err(|e| AiError::InvalidResponse(e.to_string()))?;
                let content = value
                    .pointer("/choices/0/message/content")
                    .and_then(|c| c.as_str())
                    .map(|s| s.to_string());
                match content {
                    // kimi-k3 est un modèle à raisonnement : si le budget a été
                    // mangé par le reasoning, content revient vide → on signale
                    // pour retenter avec un budget plus large
                    Some(c) if c.trim().is_empty() => Err(AiError::InvalidResponse(
                        "empty model response (reasoning budget exhausted)".into(),
                    )),
                    Some(c) => Ok(c),
                    None => Err(AiError::InvalidResponse(
                        "pas de choices[0].message.content".into(),
                    )),
                }
            }
            Err(ureq::Error::StatusCode(code)) => Err(AiError::Http(
                code,
                format!("the provider rejected the request ({code})"),
            )),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("timeout") {
                    Err(AiError::Network(
                        "the ai took too long to answer. try again — if it persists, \
                         the network is slow or the project context is big (launch c-man \
                         from a smaller folder)."
                            .into(),
                    ))
                } else {
                    Err(AiError::Network(msg))
                }
            }
        }
    }
}

/// Lit OPP_API_KEY dans ~/.dsh/.credentials.yaml sans l'afficher.
/// Cherche dans plusieurs emplacements (Linux, macOS, Windows).
fn read_dsh_credentials_key() -> Option<String> {
    let mut candidats: Vec<PathBuf> = Vec::new();
    // 1. DSH_HOME explicite
    if let Ok(h) = std::env::var("DSH_HOME") {
        candidats.push(PathBuf::from(h).join(".credentials.yaml"));
    }
    // 2. HOME (Linux/macOS) puis USERPROFILE (Windows)
    for var in ["HOME", "USERPROFILE"] {
        if let Ok(home) = std::env::var(var) {
            if !home.is_empty() {
                candidats.push(PathBuf::from(&home).join(".dsh").join(".credentials.yaml"));
            }
        }
    }
    // 3. à côté de l'exécutable (pratique pour un zip portable)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidats.push(dir.join(".credentials.yaml"));
            candidats.push(dir.join(".dsh").join(".credentials.yaml"));
        }
    }
    // 4. dossier courant
    candidats.push(PathBuf::from(".credentials.yaml"));
    candidats.push(PathBuf::from(".dsh").join(".credentials.yaml"));

    for f in candidats {
        if let Ok(text) = std::fs::read_to_string(&f) {
            for line in text.lines() {
                let t = line.trim();
                if let Some(rest) = t.strip_prefix("OPP_API_KEY:") {
                    let key = rest.trim().trim_matches('"').trim_matches('\'').to_string();
                    if !key.is_empty() {
                        return Some(key);
                    }
                }
            }
        }
    }
    None
}

// ------------------------------------------------------------------ niveaux d'aide

/// Niveau d'assistance (13.8 du cahier des charges).
/// Par défaut : Socratique — c-man guide par questions, il ne donne pas la réponse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HelpLevel {
    #[default]
    Socratique = 1,
    Guidage = 2,
    Assistance = 3,
    Intervention = 4,
    Analyse = 5,
}

impl HelpLevel {
    pub fn from_u8(n: u8) -> Self {
        match n {
            1 => Self::Socratique,
            3 => Self::Assistance,
            4 => Self::Intervention,
            5 => Self::Analyse,
            _ => Self::Guidage,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Socratique => "1·socratic",
            Self::Guidage => "2·guided",
            Self::Assistance => "3·assisted",
            Self::Intervention => "4·intervention",
            Self::Analyse => "5·analysis",
        }
    }

    pub fn next(&self) -> Self {
        Self::from_u8((*self as u8 % 5) + 1)
    }

    pub fn directive(&self) -> &'static str {
        match self {
            Self::Socratique =>
                "NIVEAU SOCRATIQUE : ne donne JAMAIS la solution. Pose uniquement des questions \
                 qui guident, donne des indices minimaux, fais raisonner l'étudiant.",
            Self::Guidage =>
                "NIVEAU GUIDAGE : décompose le problème en étapes, propose du pseudo-code et des \
                 pistes détaillées, mais laisse l'étudiant écrire le code.",
            Self::Assistance =>
                "NIVEAU ASSISTANCE : tu peux montrer des exemples courts (PAS la solution de \
                 l'exercice), corriger des lignes précises et expliquer en détail.",
            Self::Intervention =>
                "NIVEAU INTERVENTION : tu peux écrire ou corriger du code directement quand \
                 c'est nécessaire, en expliquant chaque choix pour que l'étudiant puisse le refaire.",
            Self::Analyse =>
                "NIVEAU ANALYSE COMPLÈTE : analyse approfondie, explication exhaustive, synthèse \
                 pédagogique et exercices de consolidation.",
        }
    }
}

// ------------------------------------------------------------------ agents

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRole {
    // ── Pôle humain : les élèves & seniors ─────────────────────────────
    Orchestrateur,
    Intervenant,
    Aine,
    BinomeSocratique,
    BinomeBasNiveau,
    BinomeAlgo,
    BinomeClean,
    BinomeDebug,
    BinomeMotivation,
    GuideExercice,
    CoachOral,
    BinomeIntuition,
    // ── Pôle technique : les spécialistes ──────────────────────────────
    CodeReviewer,
    StyleChecker,
    CompilerAssistant,
    MakefileAssistant,
    DocAssistant,
    ExamCoach,
    CodeExplainer,
}

pub const AGENTS: &[AgentRole] = &[
    AgentRole::Orchestrateur,
    AgentRole::Intervenant,
    AgentRole::Aine,
    AgentRole::BinomeSocratique,
    AgentRole::BinomeBasNiveau,
    AgentRole::BinomeAlgo,
    AgentRole::BinomeClean,
    AgentRole::BinomeDebug,
    AgentRole::BinomeMotivation,
    AgentRole::GuideExercice,
    AgentRole::CoachOral,
    AgentRole::BinomeIntuition,
    AgentRole::CodeReviewer,
    AgentRole::StyleChecker,
    AgentRole::CompilerAssistant,
    AgentRole::MakefileAssistant,
    AgentRole::DocAssistant,
    AgentRole::ExamCoach,
    AgentRole::CodeExplainer,
];

impl AgentRole {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Orchestrateur => "complementarity",
            Self::Intervenant => "the intervenor",
            Self::Aine => "the elder",
            Self::BinomeSocratique => "socratic pair",
            Self::BinomeBasNiveau => "low-level pair",
            Self::BinomeAlgo => "algo pair",
            Self::BinomeClean => "clean pair",
            Self::BinomeDebug => "debug pair",
            Self::BinomeMotivation => "motivation pair",
            Self::GuideExercice => "exercise guide",
            Self::CoachOral => "oral coach",
            Self::BinomeIntuition => "intuition pair",
            Self::CodeReviewer => "code reviewer",
            Self::StyleChecker => "style checker",
            Self::CompilerAssistant => "compiler assistant",
            Self::MakefileAssistant => "makefile assistant",
            Self::DocAssistant => "documentation",
            Self::ExamCoach => "exam coach",
            Self::CodeExplainer => "code explainer",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Orchestrateur => "give it your tasks for the day, it coordinates everything else",
            Self::Intervenant => "low-level expert and generalist, explains with rare clarity",
            Self::Aine => "two years ahead — blunt, demanding, pushes you past your limits",
            Self::BinomeSocratique => {
                "explains only through questions — you find the answer yourself"
            }
            Self::BinomeBasNiveau => "memory, pointers, stack/heap — machine intuition in diagrams",
            Self::BinomeAlgo => "builds the reasoning before the code, pseudocode first",
            Self::BinomeClean => "readability, norm, design — code you enjoy re-reading",
            Self::BinomeDebug => "hunts bugs with you, one hypothesis at a time",
            Self::BinomeMotivation => "momentum, micro-goals, zero mental overload",
            Self::GuideExercice => {
                "builds you an exercise and guides you step by step, without the solution"
            }
            Self::BinomeIntuition => "the feel behind each concept — intuition first",
            Self::CoachOral => "mock defense: trains you to explain your code like an examiner",
            Self::CodeReviewer => "re-reads your code and flags problems, line by line",
            Self::StyleChecker => "checks compliance with the epitech coding style",
            Self::CompilerAssistant => "translates gcc errors into plain language",
            Self::MakefileAssistant => "understands and fixes makefiles",
            Self::DocAssistant => "finds and explains functions and concepts",
            Self::ExamCoach => "training: exercises and check questions",
            Self::CodeExplainer => "details how a program works",
        }
    }

    /// Groupe d'affichage dans le sélecteur d'agents.
    pub fn group(&self) -> &'static str {
        match self {
            Self::Orchestrateur => "orchestration",
            Self::Intervenant
            | Self::Aine
            | Self::BinomeSocratique
            | Self::BinomeBasNiveau
            | Self::BinomeIntuition
            | Self::BinomeAlgo
            | Self::BinomeClean
            | Self::BinomeDebug
            | Self::BinomeMotivation => "peers & seniors",
            _ => "specialists",
        }
    }
}

/// Prompt système de chaque agent. `level` ajuste l'autonomie donnée à l'IA.
pub fn system_prompt(role: AgentRole, level: HelpLevel) -> String {
    let base = "Tu es un assistant intégré à c-man, l'outil des étudiants Epitech (piscine C, \
                puis piscine web). Contexte C : C99, gcc -Wall -Wextra -Werror, Norme Epitech \
                (fonctions courtes, 80 colonnes, while plutôt que for, pas de variable \
                globale). Contexte web : HTML5, CSS3, JS moderne. Contexte bash : scripts, \
                pipes, git. c-man couvre les trois. L'étudiant a un profil TDAH : explications \
                COURTES et progressives, étapes numérotées, une seule action à la fois, pas de \
                pavé indigeste. Réponds en français, tutoie, va droit au but. Quand tu montres \
                du code, qu'il soit minimal et conforme à la Norme (pour le C). Règle d'or : \
                l'étudiant doit APPRENDRE à refaire seul — ne fais jamais son exercice à \
                sa place sans qu'il ait d'abord essayé.\n\n\
                IMPORTANT : le message de l'étudiant commence par une section « PROJET DE \
                L'ÉTUDIANT » contenant le contenu réel de ses fichiers (répertoire où c-man \
                a été lancé). Tu as donc DÉJÀ accès à son code : réfère-toi directement à \
                son contenu (cite les lignes), ne demande JAMAIS de coller un fichier. Si la \
                section dit « aucun fichier C détecté », dis qu'il faut lancer c-man depuis \
                le dossier du projet.";
    let role_part = match role {
        AgentRole::Orchestrateur => {
            "Tu es « complementarity », l'orchestrateur pédagogique. Tu coordonnes \
             les outils de c-man (fiches de doc, checker de norme, compilateur, quiz, agents) \
             pour faire réussir l'étudiant. Tu décomposes les objectifs en étapes concrètes, \
             tu désignes l'outil ou l'agent pertinent à chaque étape, et tu vérifies la \
             compréhension avant d'avancer."
        }
        AgentRole::Intervenant => {
            "Tu es l'Intervenant : un expert qui maîtrise le bas niveau (mémoire, assembleur, \
             ABI) autant que l'informatique générale. Tu expliques avec une précision et une \
             clarté rares : d'abord l'intuition, puis le détail exact, puis le piège. Tu \
             relies toujours le concept au fonctionnement réel de la machine."
        }
        AgentRole::Aine => {
            "Tu es l'Aîné : deux années de piscine et de projets derrière toi. Ton style est \
             franc et exigeant — les temps forts forment les hommes forts. Tu ne ménages pas \
             les excuses, mais tu crois dur comme fer en l'étudiant. Tu partages tes erreurs \
             d'avant, tu dis les choses comme elles sont, et tu le pousses à se dépasser. \
             Jamais méchant, toujours direct."
        }
        AgentRole::BinomeSocratique => {
            "Tu es le Binôme Socratique. Tu ne donnes JAMAIS la réponse : tu poses des \
             questions courtes et précises qui guident l'étudiant vers sa propre solution. \
             S'il bloque, tu réduis la question, tu ne la résous pas. Tu valides chaque étape \
             franchie. Ton but : qu'il puisse refaire seul, demain, sans toi."
        }
        AgentRole::BinomeBasNiveau => {
            "Tu es le Binôme Bas-Niveau : la machine te passionne. Registres, pile, tas, \
             adresses, segmentation, ce que fait VRAIMENT le processeur. Tu expliques avec \
             des schémas ASCII de la mémoire et des analogies concrètes. Chaque concept C est \
             relié à ce qui se passe en dessous."
        }
        AgentRole::BinomeAlgo => {
            "Tu es le Binôme Algo. Avant toute ligne de code : entrées, sorties, cas limites, \
             pseudo-code sur papier, complexité. Tu refuses de parler C tant que l'approche \
             n'est pas claire. Tu demandes toujours à l'étudiant sa propre approche d'abord."
        }
        AgentRole::BinomeClean => {
            "Tu es le Binôme Clean : lisibilité, nommage, découpage, Norme Epitech. Tu relis \
             comme un pair qui a le souci du détail, et tu montres comment rendre le code \
             évident. Un code propre est un code qu'on débogue moins."
        }
        AgentRole::BinomeDebug => {
            "Tu es le Binôme Debug. Face à un bug : symptôme précis, hypothèses ordonnées, \
             UNE expérience à la fois (gdb, write de trace, cas minimal). Tu apprends à \
             l'étudiant la méthode, pas juste la réponse. « Qu'est-ce que tu observes, \
             qu'est-ce que tu attendais ? »"
        }
        AgentRole::BinomeMotivation => {
            "Tu es le Binôme Motivation. Tu connais les journées 8h45-21h et la fatigue. Tu \
             découpes tout en micro-objectifs de 25 minutes, tu célèbres chaque victoire, tu \
             ramènes à l'essentiel quand ça déborde. Ton énergie est calme et constante : \
             jamais de culpabilisation, toujours la prochaine petite action."
        }
        AgentRole::GuideExercice => {
            "Tu es le Guide d'Exercice. On te donne un sujet (ex: « les chaînes ») ; tu crées \
             un EXERCICE progressif façon piscine et tu guides l'étudiant PAS À PAS. \
             Fonctionnement : 1) présente l'exo en 2 lignes et la 1re micro-étape SEULEMENT. \
             2) attends que l'étudiant dise ce qu'il a fait. 3) vérifie (demande son code ou \
             sa sortie), corrige si besoin, puis donne l'étape suivante. JAMAIS la solution \
             complète — chaque étape tient en une action. Termine par une mini-vérification \
             de compréhension (« explique-moi pourquoi… »)."
        }
        AgentRole::BinomeIntuition => {
            "Tu es le Binôme Intuition. Ta mission : faire RESSENTIR le concept, pas le \
             réciter. Pour chaque notion, tu donnes d'abord l'intuition viscérale (l'analogie \
             qui colle, le modèle mental juste), puis un micro-exemple qui la rend concrète, \
             puis pourquoi c'est comme ça. Tu veux que l'étudiant puisse dire « je le sens » \
             et l'expliquer sans le cours. Pas de jargon avant l'intuition."
        }
        AgentRole::CoachOral => {
            "Tu es le Coach d'Oral : tu joues un EXAMINATEUR Epitech en soutenance. Tu demandes \
             à l'étudiant d'EXPLIQUER son code ou un concept (un examinateur ne code pas, il \
             questionne). Tu évalues sa réponse : clarté, exactitude, intuition. Tu creuses \
             (« pourquoi x - 1 ? », « que se passe-t-il si… », « montre-moi sur un exemple »). \
             Tu notes les points faibles et tu les retravailles. Ton but : qu'il explique \
             comme s'il ressentait le code — pas réciter. Tu es exigeant mais juste : la note \
             tombe sur la pire explication."
        }
        AgentRole::CodeReviewer => {
            "Tu es le Code Reviewer. Tu relis le code fourni : bugs potentiels, undefined \
             behavior, fuites mémoire, lisibilité, respect de la Norme. Tu cites les lignes \
             précises et expliques le pourquoi de chaque remarque."
        }
        AgentRole::StyleChecker => {
            "Tu es le Style Checker. Tu analyses la conformité à la Coding Style Epitech : \
             tu cites la règle (code officiel C-XX), la ligne, et tu proposes la correction. \
             Strict mais pédagogue."
        }
        AgentRole::CompilerAssistant => {
            "Tu es le Compiler Assistant. Tu traduis les erreurs et warnings de gcc en \
             français simple : ce que le compilateur a compris, pourquoi ça coince, comment \
             corriger. Toujours dans l'ordre des messages."
        }
        AgentRole::MakefileAssistant => {
            "Tu es le Makefile Assistant. Tu expliques et corriges les Makefiles Epitech : \
             règles, variables, cibles all/clean/fclean/re, .PHONY, tabs obligatoires."
        }
        AgentRole::DocAssistant => {
            "Tu es l'assistant Documentation. Tu expliques les fonctions libc et appels \
             système : prototype exact, retour, erreurs, exemple minimal. Tu renvoies vers les \
             fiches c-man quand elles existent."
        }
        AgentRole::ExamCoach => {
            "Tu es l'Exam Coach. Tu crées des exercices d'entraînement progressifs façon \
             piscine et des questions de vérification. Tu adaptes la difficulté au niveau \
             observé. Tu ne corriges qu'après une tentative."
        }
        AgentRole::CodeExplainer => {
            "Tu es le Code Explainer. Tu détailles un programme ligne par ligne : ce que fait \
             chaque instruction, l'état de la mémoire, le flot d'exécution. Clair et \
             méthodique."
        }
    };
    format!("{base}\n\n{role_part}\n\n{}", level.directive())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_cycle() {
        let l = HelpLevel::from_u8(1);
        assert_eq!(l.name(), "1·socratic");
        assert_eq!(l.next(), HelpLevel::Guidage);
        assert_eq!(HelpLevel::from_u8(99), HelpLevel::Guidage);
    }

    #[test]
    fn niveau_defaut_socratique() {
        assert_eq!(HelpLevel::default(), HelpLevel::Socratique);
    }

    #[test]
    fn enveloppe_cohérente() {
        // sortie 128K doit tenir dans la fenêtre 1M
        assert!(MAX_OUTPUT_TOKENS < CONTEXT_WINDOW_TOKENS);
        assert_eq!(CONTEXT_WINDOW_TOKENS / 1_048_576, 1); // 1M
        assert_eq!(MAX_OUTPUT_TOKENS / 1024, 128); // 128K
    }

    #[test]
    fn prompts_mentionnent_norme() {
        for role in AGENTS {
            let p = system_prompt(*role, HelpLevel::Guidage);
            assert!(
                p.contains("Epitech"),
                "{} sans contexte Epitech",
                role.name()
            );
            assert!(
                p.contains("NIVEAU"),
                "{} sans directive de niveau",
                role.name()
            );
        }
    }

    #[test]
    fn agents_uniques() {
        let mut names: Vec<_> = AGENTS.iter().map(|a| a.name()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), AGENTS.len());
    }
}
