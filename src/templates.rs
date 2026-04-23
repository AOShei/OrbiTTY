/// A workspace template describes which sessions get spawned when a workspace
/// is opened and which of them auto-promote into the Tiling Arena.
#[derive(Debug, Clone)]
pub struct SessionSpec {
    pub name: String,
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
                    promote: true,
                }],
            },
            WorkspaceTemplate {
                name: "Fullstack",
                sessions: vec![
                    SessionSpec {
                        name: "API".into(),
                        promote: true,
                    },
                    SessionSpec {
                        name: "UI".into(),
                        promote: true,
                    },
                    SessionSpec {
                        name: "DB".into(),
                        promote: true,
                    },
                    SessionSpec {
                        name: "Tests".into(),
                        promote: true,
                    },
                    SessionSpec {
                        name: "Cloud Logs".into(),
                        promote: false,
                    },
                    SessionSpec {
                        name: "Git Monitor".into(),
                        promote: false,
                    },
                ],
            },
            WorkspaceTemplate {
                name: "Microservices",
                sessions: vec![
                    SessionSpec {
                        name: "Gateway".into(),
                        promote: true,
                    },
                    SessionSpec {
                        name: "Auth".into(),
                        promote: true,
                    },
                    SessionSpec {
                        name: "Worker".into(),
                        promote: true,
                    },
                    SessionSpec {
                        name: "Queue".into(),
                        promote: false,
                    },
                    SessionSpec {
                        name: "Metrics".into(),
                        promote: false,
                    },
                ],
            },
            WorkspaceTemplate {
                name: "Dev",
                sessions: vec![
                    SessionSpec {
                        name: "Editor".into(),
                        promote: true,
                    },
                    SessionSpec {
                        name: "Build".into(),
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
