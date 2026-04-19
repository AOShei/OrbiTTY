/// A workspace template describes which sessions get spawned when a workspace
/// is opened and which of them auto-promote into the Tiling Arena.
#[derive(Debug, Clone)]
pub struct SessionSpec {
    pub name: String,
    pub command: Option<Vec<String>>, // None = default shell
    pub promote: bool,
}

#[derive(Debug, Clone)]
pub struct WorkspaceTemplate {
    pub name: &'static str,
    pub sessions: Vec<SessionSpec>,
}

impl WorkspaceTemplate {
    pub fn all() -> Vec<WorkspaceTemplate> {
        vec![
            WorkspaceTemplate {
                name: "Empty",
                sessions: vec![SessionSpec {
                    name: "Shell".into(),
                    command: None,
                    promote: true,
                }],
            },
            WorkspaceTemplate {
                name: "Fullstack",
                sessions: vec![
                    SessionSpec {
                        name: "API".into(),
                        command: None,
                        promote: true,
                    },
                    SessionSpec {
                        name: "UI".into(),
                        command: None,
                        promote: true,
                    },
                    SessionSpec {
                        name: "DB".into(),
                        command: None,
                        promote: true,
                    },
                    SessionSpec {
                        name: "Tests".into(),
                        command: None,
                        promote: true,
                    },
                    SessionSpec {
                        name: "Cloud Logs".into(),
                        command: None,
                        promote: false,
                    },
                    SessionSpec {
                        name: "Git Monitor".into(),
                        command: None,
                        promote: false,
                    },
                ],
            },
            WorkspaceTemplate {
                name: "Microservices",
                sessions: vec![
                    SessionSpec {
                        name: "Gateway".into(),
                        command: None,
                        promote: true,
                    },
                    SessionSpec {
                        name: "Auth".into(),
                        command: None,
                        promote: true,
                    },
                    SessionSpec {
                        name: "Worker".into(),
                        command: None,
                        promote: true,
                    },
                    SessionSpec {
                        name: "Queue".into(),
                        command: None,
                        promote: false,
                    },
                    SessionSpec {
                        name: "Metrics".into(),
                        command: None,
                        promote: false,
                    },
                ],
            },
            WorkspaceTemplate {
                name: "Dev",
                sessions: vec![
                    SessionSpec {
                        name: "Editor".into(),
                        command: None,
                        promote: true,
                    },
                    SessionSpec {
                        name: "Build".into(),
                        command: None,
                        promote: true,
                    },
                ],
            },
        ]
    }

    pub fn by_name(name: &str) -> Option<WorkspaceTemplate> {
        Self::all().into_iter().find(|t| t.name == name)
    }
}
