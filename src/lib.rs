extern crate byteorder;
#[macro_use]
extern crate lazy_static;

use std::env::args;
use std::collections::HashMap;
use byteorder::{LittleEndian, ByteOrder};

// For part parsing: if we encounter 0x02 in message, check next byte for type stored in a hashmap
// or something.

#[derive(Debug)]
pub struct RawEntry {
  pub bytes: Vec<u8>
}

impl RawEntry {
  pub fn new(bytes: Vec<u8>) -> Self {
    RawEntry {
      bytes: bytes
    }
  }

  pub fn as_parts(&self) -> Option<RawEntryParts> {
    let header = match self.get_header() {
      Some(h) => h,
      None => return None
    };
    let second_colon = match self.bytes[9..].iter().position(|b| b == &0x3a) {
      Some(i) => i,
      None => return None
    };
    let sender = self.bytes[9..second_colon + 9].to_vec();
    let message = self.bytes[second_colon + 9 + 1..].to_vec();
    Some(RawEntryParts {
      header: header,
      sender: sender,
      message: message
    })
  }

  fn get_header(&self) -> Option<Vec<u8>> {
    if self.bytes.len() < 8 {
      return None;
    }
    Some(self.bytes[..8].to_vec())
  }

  fn get_text(&self) -> Option<String> {
    let colon = match self.bytes[9..].iter().position(|b| b == &0x3a) {
      Some(i) => i,
      None => return None
    };
    Some(String::from_utf8_lossy(&self.bytes[colon + 9 + 1..]).into_owned().replace('\r', "\n"))
  }
}

#[derive(Debug)]
pub struct RawEntryParts {
  pub header: Vec<u8>,
  pub sender: Vec<u8>,
  pub message: Vec<u8>
}

impl RawEntryParts {
  pub fn as_entry(&self) -> Entry {
    let entry_type = self.header[4];
    let timestamp = LittleEndian::read_u32(&self.header[..4]);
    let sender = if self.sender.is_empty() {
      None
    } else {
      if let Some(part) = NamePart::parse(&self.sender) {
        Some(part)
      } else if let Ok(name) = String::from_utf8(self.sender.clone()) {
        Some(NamePart::from_names(&name, &name))
      } else {
        None
      }
    };
    let message = Message::new(MessageParser::parse(&self.message));
    Entry {
      entry_type: entry_type,
      timestamp: timestamp,
      sender: sender,
      message: message
    }
  }
}

#[derive(Debug)]
pub struct Entry {
  pub entry_type: u8,
  pub timestamp: u32,
  pub sender: Option<Part>,
  pub message: Message
}

#[derive(Debug)]
pub struct Message {
  pub parts: Vec<Part>
}

impl Message {
  fn new(parts: Vec<Part>) -> Self {
    Message {
      parts: parts
    }
  }
}

impl HasDisplayText for Message {
  fn display_text(&self) -> String {
    let display_texts: Vec<String> = self.parts.iter().map(|x| x.display_text()).collect();
    display_texts.join("")
  }
}

pub trait HasDisplayText {
  fn display_text(&self) -> String;
}

pub trait DeterminesLength {
  fn determine_length(bytes: &[u8]) -> usize;
}

pub trait VerifiesData {
  fn verify_data(bytes: &[u8]) -> bool;
}

pub trait Parses {
  fn parse(bytes: &[u8]) -> Option<Part>;
}

pub trait MessagePart: HasDisplayText {}

pub trait HasMarkerBytes {
  fn marker_bytes() -> (u8, u8);
}

#[derive(Debug)]
pub enum Part {
  Name { real_name: String, display_name: String },
  AutoTranslate { category: u8, id: usize },
  PlainText(String)
}

impl HasDisplayText for Part {
  fn display_text(&self) -> String {
    match *self {
      Part::PlainText(ref text) => text.clone(),
      Part::Name { real_name: _, ref display_name } => display_name.clone(),
      Part::AutoTranslate { category, id } => format!("<AT: {}, {}>", category, id)
    }
  }
}

#[derive(Debug)]
pub struct NamePart;

impl NamePart {
  fn from_names<S>(real_name: S, display_name: S) -> Part
    where S: AsRef<str>
  {
    Part::Name {
      real_name: real_name.as_ref().to_owned(),
      display_name: display_name.as_ref().to_owned()
    }
  }
}

impl HasMarkerBytes for NamePart {
  fn marker_bytes() -> (u8, u8) {
    static MARKER: (u8, u8) = (0x02, 0x27);
    MARKER
  }
}

impl VerifiesData for NamePart {
  fn verify_data(bytes: &[u8]) -> bool {
    if bytes.len() < 22 {
      return false;
    }
    let (two, marker) = NamePart::marker_bytes();
    if bytes[0] != two || bytes[1] != marker {
      return false;
    }
    return true;
  }
}

impl DeterminesLength for NamePart {
  fn determine_length(bytes: &[u8]) -> usize {
    let end_pos = match bytes[2..].windows(2).position(|w| w == &[0x02, 0x27]) {
      Some(i) => i,
      None => return 0
    };
    let last_three = match bytes[end_pos + 2..].iter().position(|b| b == &0x03) {
      Some(i) => i,
      None => return 0
    };
    let sum = 2 + end_pos + last_three;
    sum as usize
  }
}

impl Parses for NamePart {
  fn parse(bytes: &[u8]) -> Option<Part> {
    if !NamePart::verify_data(bytes) {
      return None;
    }
    let real_length = bytes[2] as usize + 2;
    let display_end = match bytes[real_length..].iter().position(|b| b == &0x02) {
      Some(i) => i + real_length,
      None => return None
    };
    let real_name = match String::from_utf8(bytes[9..real_length].to_vec()) {
      Ok(r) => r,
      Err(_) => return None
    };
    let display_name = match String::from_utf8(bytes[real_length + 1 .. display_end].to_vec()) {
      Ok(d) => d,
      Err(_) => return None
    };
    Some(NamePart::from_names(real_name, display_name))
  }
}

pub struct AutoTranslatePart;

impl AutoTranslatePart {
  fn from_parts(category: u8, id: usize) -> Part {
    Part::AutoTranslate {
      category: category,
      id: id
    }
  }

  fn byte_array_to_be(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 1 {
      return None;
    }
    if bytes.len() == 1 {
      return Some(bytes[0] as usize);
    }
    let length = bytes.len();
    let mut res: usize = (bytes[0] as usize) << (8 * (length - 1));
    for (i, b) in bytes[1..].iter().enumerate() {
      let bits = 8 * (length - i - 2);
      res |= (*b as usize) << bits
    }
    Some(res)
  }
}

impl HasMarkerBytes for AutoTranslatePart {
  fn marker_bytes() -> (u8, u8) {
    static MARKER: (u8, u8) = (0x02, 0x2e);
    MARKER
  }
}

impl VerifiesData for AutoTranslatePart {
  fn verify_data(bytes: &[u8]) -> bool {
    if bytes.len() < 6 {
      return false;
    }
    let (two, marker) = AutoTranslatePart::marker_bytes();
    if bytes[0] != two || bytes[1] != marker {
      return false;
    }
    return true;
  }
}

impl DeterminesLength for AutoTranslatePart {
  fn determine_length(bytes: &[u8]) -> usize {
    bytes[2] as usize + 3
  }
}

impl Parses for AutoTranslatePart {
  fn parse(bytes: &[u8]) -> Option<Part> {
    if !AutoTranslatePart::verify_data(bytes) {
      return None;
    }
    let length = bytes[2];
    let category = bytes[3];
    let id = match AutoTranslatePart::byte_array_to_be(&bytes[4..3 + length as usize]) {
      Some(id) => id,
      None => return None
    };
    Some(AutoTranslatePart::from_parts(category, id))
  }
}

macro_rules! parse_structure_macro {
  ($t:ident, $message:expr) => {{
    let length = $t::determine_length(&$message);
    let part = match $t::parse(&$message[..length]) {
      Some(p) => p,
      None => return None
    };
    Some((length, part))
  }};
}

struct PlainTextPart;

impl PlainTextPart {
  fn new<S>(text: S) -> Part
    where S: AsRef<str>
  {
    Part::PlainText(text.as_ref().to_owned())
  }
}

pub struct MessageParser;

impl MessageParser {
  pub fn parse(message: &[u8]) -> Vec<Part> {
    let mut parts: Vec<Part> = Vec::new();
    let mut buf: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < message.len() {
      let byte = message[i];
      if byte == 0x02 {
        if let Some((len, part)) = MessageParser::parse_structure(&message[i..]) {
          if !buf.is_empty() {
            parts.push(PlainTextPart::new(String::from_utf8_lossy(&buf)));
            buf.clear();
          }
          parts.push(part);
          i += len + 1;
          continue;
        }
      }
      buf.push(byte);
      i += 1;
    }
    if !buf.is_empty() {
      parts.push(PlainTextPart::new(String::from_utf8_lossy(&buf)));
    }
    parts
  }

  fn parse_structure(message: &[u8]) -> Option<(usize, Part)> {
    if message.len() < 2 {
      return None;
    }
    let structure_id = message[1];
    if structure_id == NamePart::marker_bytes().1 {
      parse_structure_macro!(NamePart, message)
    } else if structure_id == AutoTranslatePart::marker_bytes().1 {
      parse_structure_macro!(AutoTranslatePart, message)
    } else {
      None
    }
  }
}