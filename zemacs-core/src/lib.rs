pub use encoding_rs as encoding;

pub mod abbrev;
pub mod arglist;
pub mod auto_pairs;
pub mod bookmark;
pub mod buffer_menu;
pub mod buffer_name;
pub mod calc;
pub mod calendar;
pub mod case_conversion;
pub mod changelog;
pub mod chars;
pub mod cmode;
pub mod comint;
pub mod command_line;
pub mod comment;
pub mod compilation;
pub mod completion;
pub mod config;
pub mod cursor_info;
pub mod desktop;
pub mod diagnostic;
pub mod diary;
pub mod diff;
pub mod diffmode;
pub mod dired;
pub mod doc_formatter;
pub mod editor_config;
pub mod email;
pub mod etags;
pub mod facemenu;
pub mod ffap;
pub mod file_locals;
pub mod fold;
pub mod fortran;
pub mod fuzzy;
pub mod graphemes;
pub mod history;
pub mod icalendar;
pub mod increment;
pub mod indent;
pub mod injection;
pub mod ispell;
pub mod kbd;
pub mod kmacro;
pub mod line_ending;
pub mod line_filter;
pub mod list_motion;
pub mod macros;
pub mod match_brackets;
pub mod merge_ops;
pub mod movement;
pub mod object;
pub mod occur;
pub mod outline;
pub mod page;
pub mod picture;
mod position;
pub mod power_edit;
pub mod proced;
pub mod project;
pub mod ps_print;
pub mod query_replace;
pub mod quickfix;
pub mod region_ops;
pub mod rmail;
pub mod search;
pub mod selection;
pub mod selective_display;
pub mod sgml;
pub mod snippets;
pub mod sort;
pub mod sort_subr;
pub mod surround;
pub mod syntax;
pub mod table;
pub mod test;
pub mod tex;
pub mod text_annotations;
pub mod text_engine;
pub mod textobject;
pub mod time_stamp;
pub mod timeclock;
mod transaction;
pub mod two_column;
pub mod uri;
pub mod vc;
pub mod vim_opts;
pub mod whitespace;
pub mod wrap;
pub mod xref;

pub mod unicode {
    pub use unicode_general_category as category;
    pub use unicode_segmentation as segmentation;
    pub use unicode_width as width;
}

pub use zemacs_loader::find_workspace;

mod rope_reader;

pub use rope_reader::RopeReader;
pub use ropey::{self, str_utils, Rope, RopeBuilder, RopeSlice};

// pub use tendril::StrTendril as Tendril;
pub use smartstring::SmartString;

pub type Tendril = SmartString<smartstring::LazyCompact>;

#[doc(inline)]
pub use {regex, tree_house::tree_sitter};

pub use position::{
    char_idx_at_visual_offset, coords_at_pos, pos_at_coords, softwrapped_dimensions,
    visual_offset_from_anchor, visual_offset_from_block, Position, VisualOffsetError,
};
#[allow(deprecated)]
pub use position::{pos_at_visual_coords, visual_coords_at_pos};

pub use selection::{Range, Selection};
pub use smallvec::{smallvec, SmallVec};
pub use syntax::Syntax;

pub use completion::CompletionItem;
pub use diagnostic::Diagnostic;

pub use line_ending::{LineEnding, NATIVE_LINE_ENDING};
pub use transaction::{Assoc, Change, ChangeSet, Deletion, Operation, Transaction};

pub use uri::Uri;

pub use tree_house::Language;
