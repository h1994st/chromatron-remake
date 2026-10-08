use object::{Architecture, Object, ObjectSection};
use sha2::{Digest, Sha256};

use crate::{DecodeError, Game};

/// One Mach-O section. `data` borrows the binary; zero-fill sections have empty data.
#[derive(Debug)]
pub struct Section<'a> {
    pub segment: String,
    pub name: String,
    pub vmaddr: u32,
    pub data: &'a [u8],
}

/// A verified original binary, addressable by PowerPC virtual address.
#[derive(Debug)]
pub struct Image<'a> {
    game: Game,
    sections: Vec<Section<'a>>,
}

impl<'a> Image<'a> {
    /// Verifies `bytes` against the game's pinned SHA-256, then indexes its sections.
    pub fn load(game: Game, bytes: &'a [u8]) -> Result<Image<'a>, DecodeError> {
        let found = hex(Sha256::digest(bytes).as_slice());
        if found != game.sha256() {
            return Err(DecodeError::HashMismatch { game, found });
        }
        let parse = |e: object::Error| DecodeError::Parse {
            game,
            message: e.to_string(),
        };
        let file = object::File::parse(bytes).map_err(parse)?;
        if file.architecture() != Architecture::PowerPc {
            return Err(DecodeError::NotPowerPc { game });
        }
        let mut sections = Vec::new();
        for section in file.sections() {
            sections.push(Section {
                segment: section
                    .segment_name()
                    .map_err(parse)?
                    .unwrap_or("")
                    .to_owned(),
                name: section.name().map_err(parse)?.to_owned(),
                vmaddr: section.address() as u32,
                data: section.data().map_err(parse)?,
            });
        }
        Ok(Image { game, sections })
    }

    pub fn game(&self) -> Game {
        self.game
    }

    pub fn sections(&self) -> &[Section<'a>] {
        &self.sections
    }

    pub fn section(
        &self,
        segment: &'static str,
        name: &'static str,
    ) -> Result<&Section<'a>, DecodeError> {
        self.sections
            .iter()
            .find(|s| s.segment == segment && s.name == name)
            .ok_or(DecodeError::MissingSection {
                game: self.game,
                segment,
                section: name,
            })
    }

    /// `len` bytes at `vmaddr`, if they lie entirely within one section's file data.
    pub fn read(&self, vmaddr: u32, len: usize) -> Option<&'a [u8]> {
        self.sections.iter().find_map(|s| {
            let start = vmaddr.checked_sub(s.vmaddr)? as usize;
            s.data.get(start..start.checked_add(len)?)
        })
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
