//! Limites da fila de comandos dos loops: a rede é não confiável e um peer pode inundar.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Mensagens de rede aguardando o loop. Acima disso, novas mensagens são descartadas;
/// abertura e fechamento de peer nunca são.
pub const MAX_QUEUED_RECV: usize = 1024;
/// Comandos processados por passagem do loop, para uma enxurrada não impedir os ticks.
pub const MAX_COMMANDS_PER_PASS: usize = 2048;

/// Contador das mensagens de rede em fila, compartilhado entre o handle e o loop.
#[derive(Clone, Default)]
pub struct RecvBacklog(Arc<AtomicUsize>);

impl RecvBacklog {
    /// Reserva uma vaga; `false` se a fila já está cheia e a mensagem deve ser descartada.
    pub fn try_acquire(&self) -> bool {
        if self.0.fetch_add(1, Ordering::AcqRel) < MAX_QUEUED_RECV {
            true
        } else {
            self.0.fetch_sub(1, Ordering::AcqRel);
            false
        }
    }

    /// Libera a vaga quando o loop retira a mensagem da fila. Só as mensagens que passaram
    /// por `try_acquire` entram na fila, então o contador nunca fica negativo.
    pub fn release(&self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_and_releases() {
        let b = RecvBacklog::default();
        for _ in 0..MAX_QUEUED_RECV {
            assert!(b.try_acquire());
        }
        assert!(!b.try_acquire());
        b.release();
        assert!(b.try_acquire());
    }
}
