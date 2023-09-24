use std::{collections::BTreeMap, iter::Peekable};

use bitcoin::{
    blockdata::{
        opcodes,
        script::{self, Instruction, Instructions},
    },
    taproot::TAPROOT_ANNEX_PREFIX,
    Script, Witness,
};

use crate::block::Tx;

const PROTOCOL_ID: [u8; 3] = *b"ord";
const BODY_TAG: [u8; 0] = [];
const CONTENT_TYPE_TAG: [u8; 1] = [1];

#[derive(Debug, PartialEq, Clone)]
pub enum Curse {
    NotInFirstInput,
    NotAtOffsetZero,
    Reinscription,
}

#[derive(thiserror::Error, Debug, PartialEq)]
pub enum InscriptionError {
    #[error("empty witness")]
    EmptyWitness,
    #[error("invalid inscription")]
    InvalidInscription,
    #[error("key-path spend")]
    KeyPathSpend,
    #[error("no inscription")]
    NoInscription,
    #[error("script error")]
    Script(script::Error),
    #[error("unrecognized even field")]
    UnrecognizedEvenField,
}

#[derive(Debug, PartialEq, Clone)]
pub struct Inscription {
    pub body: Option<Vec<u8>>,
    pub content_type: Option<Vec<u8>>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct TransactionInscription {
    pub inscription: Inscription,
    pub tx_in_index: u32,
    pub tx_in_offset: u32,
}

impl Inscription {
    pub fn from_transaction(tx: &Tx) -> Vec<TransactionInscription> {
        let mut result = Vec::new();
        for (index, tx_in) in tx.value.inputs.iter().enumerate() {
            if tx_in.witness.is_none() {
                continue;
            }

            let Ok(inscriptions) = InscriptionParser::parse(&tx_in.witness.clone().unwrap()) else { continue };

            result.extend(
                inscriptions
                    .into_iter()
                    .enumerate()
                    .map(|(offset, inscription)| TransactionInscription {
                        inscription,
                        tx_in_index: u32::try_from(index).unwrap(),
                        tx_in_offset: u32::try_from(offset).unwrap(),
                    })
                    .collect::<Vec<TransactionInscription>>(),
            )
        }

        result
    }
}

type Result<T, E = InscriptionError> = std::result::Result<T, E>;

struct InscriptionParser<'a> {
    instructions: Peekable<Instructions<'a>>,
}

impl<'a> InscriptionParser<'a> {
    fn parse(witness: &Witness) -> Result<Vec<Inscription>> {
        if witness.is_empty() {
            return Err(InscriptionError::EmptyWitness);
        }

        if witness.len() == 1 {
            return Err(InscriptionError::KeyPathSpend);
        }

        let annex = witness
            .last()
            .and_then(|element| element.first().map(|byte| *byte == TAPROOT_ANNEX_PREFIX))
            .unwrap_or(false);

        if witness.len() == 2 && annex {
            return Err(InscriptionError::KeyPathSpend);
        }

        let script = witness
            .iter()
            .nth(if annex {
                witness.len() - 1
            } else {
                witness.len() - 2
            })
            .unwrap();

        InscriptionParser {
            instructions: Script::from_bytes(script).instructions().peekable(),
        }
        .parse_inscriptions()
        .into_iter()
        .collect()
    }

    fn parse_inscriptions(&mut self) -> Vec<Result<Inscription>> {
        let mut inscriptions = Vec::new();
        loop {
            let current = self.parse_one_inscription();
            if current == Err(InscriptionError::NoInscription) {
                break;
            }
            inscriptions.push(current);
        }

        inscriptions
    }

    fn parse_one_inscription(&mut self) -> Result<Inscription> {
        self.advance_into_inscription_envelope()?;

        let mut fields = BTreeMap::new();

        loop {
            match self.advance()? {
                Instruction::PushBytes(tag) if tag.as_bytes() == BODY_TAG.as_slice() => {
                    let mut body = Vec::new();
                    while !self.accept(&Instruction::Op(opcodes::all::OP_ENDIF))? {
                        body.extend_from_slice(self.expect_push()?);
                    }
                    fields.insert(BODY_TAG.as_slice(), body);
                    break;
                }
                Instruction::PushBytes(tag) => {
                    if fields.contains_key(tag.as_bytes()) {
                        return Err(InscriptionError::InvalidInscription);
                    }
                    fields.insert(tag.as_bytes(), self.expect_push()?.to_vec());
                }
                Instruction::Op(opcodes::all::OP_ENDIF) => break,
                _ => return Err(InscriptionError::InvalidInscription),
            }
        }

        let body = fields.remove(BODY_TAG.as_slice());
        let content_type = fields.remove(CONTENT_TYPE_TAG.as_slice());

        for tag in fields.keys() {
            if let Some(lsb) = tag.first() {
                if lsb % 2 == 0 {
                    return Err(InscriptionError::UnrecognizedEvenField);
                }
            }
        }

        Ok(Inscription { body, content_type })
    }

    fn advance(&mut self) -> Result<Instruction<'a>> {
        self.instructions
            .next()
            .ok_or(InscriptionError::NoInscription)?
            .map_err(InscriptionError::Script)
    }

    fn advance_into_inscription_envelope(&mut self) -> Result<()> {
        let inscription_envelope_header = [
            Instruction::PushBytes((&[]).into()), // This is an OP_FALSE
            Instruction::Op(opcodes::all::OP_IF),
            Instruction::PushBytes((&PROTOCOL_ID).into()),
        ];
        loop {
            if self.match_instructions(&inscription_envelope_header)? {
                break;
            }
        }

        Ok(())
    }

    fn match_instructions(&mut self, instructions: &[Instruction]) -> Result<bool> {
        for instruction in instructions {
            if &self.advance()? != instruction {
                return Ok(false);
            }
        }

        Ok(true)
    }

    fn expect_push(&mut self) -> Result<&'a [u8]> {
        match self.advance()? {
            Instruction::PushBytes(bytes) => Ok(bytes.as_bytes()),
            _ => Err(InscriptionError::InvalidInscription),
        }
    }

    fn accept(&mut self, instruction: &Instruction) -> Result<bool> {
        match self.instructions.peek() {
            Some(Ok(next)) => {
                if next == instruction {
                    self.advance()?;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Some(Err(err)) => Err(InscriptionError::Script(*err)),
            None => Ok(false),
        }
    }
}
