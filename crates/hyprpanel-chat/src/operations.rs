//! Politique partagée par les commandes, les fichiers et les actions MCP.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum Mode {
    #[default]
    ConfirmAll = 0,
    FullAccess = 1,
    SudoOnly = 2,
    CopyOnly = 3,
}

impl Mode {
    pub fn from_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::ConfirmAll),
            1 => Some(Self::FullAccess),
            2 => Some(Self::SudoOnly),
            3 => Some(Self::CopyOnly),
            _ => None,
        }
    }
    pub fn index(self) -> i32 {
        self as i32
    }
    pub fn can_operate(self) -> bool {
        self != Self::CopyOnly
    }
    pub fn confirm_command(self, command: &str) -> bool {
        match self {
            Self::ConfirmAll => true,
            Self::SudoOnly => privileged_or_indirect(command),
            Self::FullAccess | Self::CopyOnly => false,
        }
    }
    pub fn confirm_desktop(self) -> bool {
        self == Self::ConfirmAll
    }
}

// Une commande opaque peut déléguer un sudo : elle ne doit pas bénéficier
// d'une exemption fondée sur une simple absence du mot dans le script affiché.
fn privileged_or_indirect(command: &str) -> bool {
    if command.contains(['$', '`']) {
        return true;
    }
    // shlex traite les retours à la ligne comme des espaces ; conserver leur
    // rôle de séparation de commandes avant de retirer guillemets/échappements.
    let separated = command.replace('\n', "\n; ");
    let Some(words) = shlex::split(&separated) else {
        return true;
    };
    let mut command_position = true;
    for word in &words {
        for chunk in word.split_inclusive([';', '|', '&', '(', ')', '\n', '{', '}']) {
            let part = chunk.trim_end_matches([';', '|', '&', '(', ')', '\n', '{', '}']);
            let separator = part.len() != chunk.len();
            if command_position && !part.is_empty() {
                // Affectations et wrappers simples précèdent l'exécutable.
                // Les wrappers à arguments ambigus passent par la confirmation.
                if part.contains('=') {
                    continue;
                }
                if part.starts_with('-') || part.contains(['<', '>']) {
                    return true;
                }
                let name = part.rsplit('/').next().unwrap_or(part);
                if matches!(
                    name,
                    "sudo"
                        | "sudoedit"
                        | "doas"
                        | "pkexec"
                        | "su"
                        | "runuser"
                        | "eval"
                        | "source"
                        | "."
                        | "sh"
                        | "bash"
                        | "zsh"
                        | "fish"
                        | "dash"
                        | "python"
                        | "python3"
                        | "perl"
                        | "ruby"
                        | "node"
                        | "xargs"
                        | "parallel"
                        | "nice"
                        | "timeout"
                ) || part.ends_with(".sh")
                {
                    return true;
                }
                if !matches!(
                    name,
                    "env"
                        | "exec"
                        | "command"
                        | "nohup"
                        | "if"
                        | "then"
                        | "else"
                        | "do"
                        | "while"
                        | "until"
                        | "!"
                        | "time"
                ) && !part.starts_with('-')
                {
                    command_position = false;
                }
            }
            if separator {
                command_position = true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sudo_and_indirect_execution_require_confirmation_but_plain_commands_do_not() {
        for command in [
            "sudo pacman -S kitty",
            "printf ok;sudo -n true",
            "/usr/bin/sudo -n true",
            "su''do -n true",
            "su\\do -n true",
            "env X=1 sudo true",
            "env -u USER sudo true",
            "printf ok\nsudo true",
            "2>/dev/null sudo true",
            "time sudo true",
            "echo ok | sudo tee /etc/test",
            "sh -c 'id'",
            "bash script",
            "python3 script.py",
            "eval id",
            "$cmd true",
            "`which sudo` true",
            "./setup.sh",
        ] {
            assert!(Mode::SudoOnly.confirm_command(command), "{command}");
        }
        for command in [
            "id",
            "ls -la ~/.config",
            "LC_ALL=C rpm -qa",
            "printf ok && ls",
            "pacman -Q | sort",
            "cat ~/.config/app/config.toml",
            "echo sudo",
            "printf '%s' bash",
            "rg sudo app.conf",
        ] {
            assert!(!Mode::SudoOnly.confirm_command(command), "{command}");
        }
        assert!(!Mode::FullAccess.confirm_command("sudo id"));
        assert!(Mode::ConfirmAll.confirm_command("id"));
        assert!(!Mode::CopyOnly.can_operate());
    }
}
