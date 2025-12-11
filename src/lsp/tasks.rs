use std::{
    fmt,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum DocumentTaskKind {
    Completion,
    Hover,
    Analysis,
}

#[derive(Debug ,Default)]
pub struct DocumentTaskState {
    completion: Arc<TaskGeneration>,
    hover: Arc<TaskGeneration>,
    analysis: Arc<TaskGeneration>,
}

impl DocumentTaskState {
    pub fn token(&self, kind: DocumentTaskKind) -> DocumentTaskToken {
        let slot = self.slot(kind);
        DocumentTaskToken::new(kind, Arc::clone(slot))
    }

    fn slot(&self, kind: DocumentTaskKind) -> &Arc<TaskGeneration> {
        match kind {
            DocumentTaskKind::Completion => &self.completion,
            DocumentTaskKind::Hover => &self.hover,
            DocumentTaskKind::Analysis => &self.analysis,
        }
    }
}

#[derive(Debug, Default)]
struct TaskGeneration {
    generation: AtomicU64,
}

impl TaskGeneration {
    fn next_token(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn is_cancelled(&self, expected: u64) -> bool {
        self.generation.load(Ordering::SeqCst) != expected
    }

    fn cancel(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Clone)]
pub struct DocumentTaskToken {
    kind: DocumentTaskKind,
    state: Arc<TaskGeneration>,
    generation: u64,
}

impl DocumentTaskToken {
    fn new(kind: DocumentTaskKind, state: Arc<TaskGeneration>) -> Self {
        let generation = state.next_token();
        Self {
            kind,
            state,
            generation,
        }
    }

    pub fn kind(&self) -> DocumentTaskKind {
        self.kind
    }

    pub fn ensure_active(&self) -> Result<(), DocumentTaskCancelled> {
        if self.is_cancelled() {
            Err(DocumentTaskCancelled {
                kind: self.kind,
            })
        } else {
            Ok(())
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.state.is_cancelled(self.generation)
    }

    pub fn cancel(&self) {
        self.state.cancel();
    }
}

#[derive(Debug)]
pub struct DocumentTaskCancelled {
    kind: DocumentTaskKind,
}

impl fmt::Display for DocumentTaskCancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} task cancelled", self.kind)
    }
}

impl std::error::Error for DocumentTaskCancelled {}
