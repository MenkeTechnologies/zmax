//! Calc — the zemacs port of GNU Emacs `calc-mode`, the RPN stack calculator.
//!
//! Two layers live here, both pure and dependency-free:
//!
//!   * [`eval_infix`] / [`format_value`] — the algebraic (infix) evaluator that
//!     also backs the `:calc <expr>` typed command. Grammar (lowest→highest):
//!     `expr = term (('+'|'-') term)*`, `term = power (('*'|'/'|'%') power)*`,
//!     `power = factor ('^' power)?`, `factor = number | '(' expr ')' | fn '(' expr ')'
//!     | ('+'|'-') factor`. Unary minus binds tighter than `^` (so `-2^2` == 4).
//!
//!   * [`Calc`] — the RPN stack machine that the full-screen Calc Component drives.
//!     Emacs Calc keeps a stack (level 1 = top) plus a scrolling "trail" of past
//!     results, and every mutation is undoable. This mirrors that: `enter`
//!     duplicates the top (`calc-enter`), `pop` drops it (`calc-pop`), `roll_down`
//!     / `roll_up` rotate the stack (`TAB` / `M-TAB`), binary ops consume the top
//!     two levels, unary ops transform the top one, and `undo` / `redo` walk the
//!     history. Trig honours the [`Angle`] mode, matching Calc's `m d` / `m r`.

// --------------------------------------------------------------------------
// Algebraic (infix) evaluator — shared with the `:calc` typed command.
// --------------------------------------------------------------------------

/// A recursive-descent infix evaluator over `f64`.
struct Infix<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Infix<'a> {
    fn new(s: &'a str) -> Self {
        Infix {
            s: s.as_bytes(),
            i: 0,
        }
    }
    fn peek(&mut self) -> Option<u8> {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
        self.s.get(self.i).copied()
    }
    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn expr(&mut self) -> Result<f64, String> {
        let mut v = self.term()?;
        loop {
            match self.peek() {
                Some(b'+') => {
                    self.i += 1;
                    v += self.term()?;
                }
                Some(b'-') => {
                    self.i += 1;
                    v -= self.term()?;
                }
                _ => break,
            }
        }
        Ok(v)
    }
    fn term(&mut self) -> Result<f64, String> {
        let mut v = self.power()?;
        loop {
            match self.peek() {
                Some(b'*') => {
                    self.i += 1;
                    v *= self.power()?;
                }
                Some(b'/') => {
                    self.i += 1;
                    let d = self.power()?;
                    if d == 0.0 {
                        return Err("division by zero".into());
                    }
                    v /= d;
                }
                Some(b'%') => {
                    self.i += 1;
                    let d = self.power()?;
                    if d == 0.0 {
                        return Err("modulo by zero".into());
                    }
                    v %= d;
                }
                _ => break,
            }
        }
        Ok(v)
    }
    fn power(&mut self) -> Result<f64, String> {
        let base = self.factor()?;
        if self.eat(b'^') {
            Ok(base.powf(self.power()?)) // right-associative
        } else {
            Ok(base)
        }
    }
    fn factor(&mut self) -> Result<f64, String> {
        match self.peek() {
            Some(b'-') => {
                self.i += 1;
                Ok(-self.factor()?)
            }
            Some(b'+') => {
                self.i += 1;
                self.factor()
            }
            Some(b'(') => {
                self.i += 1;
                let v = self.expr()?;
                if !self.eat(b')') {
                    return Err("missing closing parenthesis".into());
                }
                Ok(v)
            }
            Some(c) if c.is_ascii_alphabetic() => self.name(),
            Some(c) if c.is_ascii_digit() || c == b'.' => self.number(),
            Some(c) => Err(format!("unexpected character '{}'", c as char)),
            None => Err("unexpected end of expression".into()),
        }
    }
    /// A bare identifier — either a constant (`pi`, `e`) or a one-argument
    /// function call (`sqrt(x)`, `sin(x)`, …), matching Calc's algebraic names.
    fn name(&mut self) -> Result<f64, String> {
        self.peek();
        let start = self.i;
        while self.i < self.s.len() && self.s[self.i].is_ascii_alphabetic() {
            self.i += 1;
        }
        let name = std::str::from_utf8(&self.s[start..self.i])
            .unwrap_or("")
            .to_ascii_lowercase();
        match name.as_str() {
            "pi" => return Ok(std::f64::consts::PI),
            "e" if self.peek() != Some(b'(') => return Ok(std::f64::consts::E),
            _ => {}
        }
        if !self.eat(b'(') {
            return Err(format!("unknown name '{name}'"));
        }
        let arg = self.expr()?;
        if !self.eat(b')') {
            return Err("missing closing parenthesis".into());
        }
        match name.as_str() {
            "sqrt" => Ok(arg.sqrt()),
            "abs" => Ok(arg.abs()),
            "ln" => Ok(arg.ln()),
            "log" => Ok(arg.log10()),
            "exp" => Ok(arg.exp()),
            "sin" => Ok(arg.sin()),
            "cos" => Ok(arg.cos()),
            "tan" => Ok(arg.tan()),
            "asin" => Ok(arg.asin()),
            "acos" => Ok(arg.acos()),
            "atan" => Ok(arg.atan()),
            "floor" => Ok(arg.floor()),
            "ceil" => Ok(arg.ceil()),
            "round" => Ok(arg.round()),
            _ => Err(format!("unknown function '{name}'")),
        }
    }
    fn number(&mut self) -> Result<f64, String> {
        self.peek();
        let start = self.i;
        while self.i < self.s.len() {
            let c = self.s[self.i];
            if c.is_ascii_digit() || c == b'.' {
                self.i += 1;
            } else if (c == b'e' || c == b'E')
                && self.i + 1 < self.s.len()
                && (self.s[self.i + 1] == b'+' || self.s[self.i + 1] == b'-')
            {
                self.i += 2; // signed exponent
            } else if c == b'e' || c == b'E' {
                self.i += 1;
            } else {
                break;
            }
        }
        let tok = std::str::from_utf8(&self.s[start..self.i]).unwrap_or("");
        tok.parse::<f64>()
            .map_err(|_| format!("invalid number '{tok}'"))
    }
}

/// Evaluate an infix arithmetic expression, erroring on trailing/garbage input.
pub fn eval_infix(input: &str) -> Result<f64, String> {
    let mut c = Infix::new(input);
    let v = c.expr()?;
    if c.peek().is_some() {
        return Err("unexpected trailing input".into());
    }
    if !v.is_finite() {
        return Err("result is not finite".into());
    }
    Ok(v)
}

/// Render a value: integers without a decimal point, otherwise trimmed.
pub fn format_value(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.10}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

// --------------------------------------------------------------------------
// RPN stack machine — the Calc Component's engine.
// --------------------------------------------------------------------------

/// Angular mode for the trig functions (Calc `m d` / `m r`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Angle {
    Radians,
    Degrees,
}

/// Binary operators consuming the top two stack levels (level 2 `op` level 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Mod,
}

/// Unary operators transforming the top stack level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Recip,
    Sqrt,
    Square,
    Abs,
    Ln,
    Exp,
    Log10,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Factorial,
}

/// The Calc RPN stack machine. Level 1 is the top of the stack (the last
/// element of `stack`), matching Calc's numbering where the newest value is `1:`.
pub struct Calc {
    stack: Vec<f64>,
    trail: Vec<String>,
    undo: Vec<Vec<f64>>,
    redo: Vec<Vec<f64>>,
    pub angle: Angle,
}

impl Default for Calc {
    fn default() -> Self {
        Self::new()
    }
}

impl Calc {
    pub fn new() -> Self {
        Calc {
            stack: Vec::new(),
            trail: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            angle: Angle::Radians,
        }
    }

    /// The stack, level 1 (top) last. Callers render it bottom-aligned.
    pub fn stack(&self) -> &[f64] {
        &self.stack
    }

    /// The trail (log of pushed results), oldest first.
    pub fn trail(&self) -> &[String] {
        &self.trail
    }

    /// Top-of-stack value (level 1), if any.
    pub fn top(&self) -> Option<f64> {
        self.stack.last().copied()
    }

    /// Snapshot the stack before a mutation so [`undo`] can restore it. Any new
    /// action invalidates the redo history, exactly as Calc's undo ring does.
    fn checkpoint(&mut self) {
        self.undo.push(self.stack.clone());
        self.redo.clear();
    }

    /// Push a value onto the stack and log it to the trail.
    pub fn push(&mut self, v: f64) {
        self.checkpoint();
        self.stack.push(v);
        self.trail.push(format_value(v));
    }

    /// `calc-enter` (RET with empty entry): duplicate the top of the stack.
    pub fn enter(&mut self) {
        if let Some(&v) = self.stack.last() {
            self.checkpoint();
            self.stack.push(v);
        }
    }

    /// `calc-pop` (DEL): drop the top of the stack.
    pub fn pop(&mut self) {
        if !self.stack.is_empty() {
            self.checkpoint();
            self.stack.pop();
        }
    }

    /// `TAB` (`calc-roll-down`): rotate the whole stack toward the top so the
    /// former top sinks to the bottom.
    pub fn roll_down(&mut self) {
        if self.stack.len() >= 2 {
            self.checkpoint();
            let top = self.stack.pop().unwrap();
            self.stack.insert(0, top);
        }
    }

    /// `M-TAB` (`calc-roll-up`): the inverse of [`roll_down`].
    pub fn roll_up(&mut self) {
        if self.stack.len() >= 2 {
            self.checkpoint();
            let bottom = self.stack.remove(0);
            self.stack.push(bottom);
        }
    }

    /// Apply a binary operator to levels 2 and 1 (`2: a  1: b` → `a op b`).
    /// Returns Err (without mutating) when the stack is too short or the
    /// operation is undefined (division/modulo by zero).
    pub fn binop(&mut self, op: BinOp) -> Result<(), String> {
        if self.stack.len() < 2 {
            return Err("stack underflow".into());
        }
        let b = self.stack[self.stack.len() - 1];
        let a = self.stack[self.stack.len() - 2];
        let r = match op {
            BinOp::Add => a + b,
            BinOp::Sub => a - b,
            BinOp::Mul => a * b,
            BinOp::Div => {
                if b == 0.0 {
                    return Err("division by zero".into());
                }
                a / b
            }
            BinOp::Pow => a.powf(b),
            BinOp::Mod => {
                if b == 0.0 {
                    return Err("modulo by zero".into());
                }
                a % b
            }
        };
        if !r.is_finite() {
            return Err("result is not finite".into());
        }
        self.checkpoint();
        self.stack.truncate(self.stack.len() - 2);
        self.stack.push(r);
        self.trail.push(format_value(r));
        Ok(())
    }

    /// Apply a unary operator to level 1.
    pub fn unop(&mut self, op: UnOp) -> Result<(), String> {
        let &x = self.stack.last().ok_or("stack underflow")?;
        let to_rad = |v: f64| match self.angle {
            Angle::Radians => v,
            Angle::Degrees => v.to_radians(),
        };
        let from_rad = |v: f64| match self.angle {
            Angle::Radians => v,
            Angle::Degrees => v.to_degrees(),
        };
        let r = match op {
            UnOp::Neg => -x,
            UnOp::Recip => {
                if x == 0.0 {
                    return Err("division by zero".into());
                }
                1.0 / x
            }
            UnOp::Sqrt => {
                if x < 0.0 {
                    return Err("sqrt of a negative".into());
                }
                x.sqrt()
            }
            UnOp::Square => x * x,
            UnOp::Abs => x.abs(),
            UnOp::Ln => {
                if x <= 0.0 {
                    return Err("ln of a non-positive".into());
                }
                x.ln()
            }
            UnOp::Log10 => {
                if x <= 0.0 {
                    return Err("log of a non-positive".into());
                }
                x.log10()
            }
            UnOp::Exp => x.exp(),
            UnOp::Sin => to_rad(x).sin(),
            UnOp::Cos => to_rad(x).cos(),
            UnOp::Tan => to_rad(x).tan(),
            UnOp::Asin => from_rad(x.asin()),
            UnOp::Acos => from_rad(x.acos()),
            UnOp::Atan => from_rad(x.atan()),
            UnOp::Factorial => factorial(x)?,
        };
        if !r.is_finite() {
            return Err("result is not finite".into());
        }
        self.checkpoint();
        *self.stack.last_mut().unwrap() = r;
        self.trail.push(format_value(r));
        Ok(())
    }

    /// Evaluate an algebraic expression (Calc `'`) and push its result.
    pub fn algebraic(&mut self, expr: &str) -> Result<(), String> {
        let v = eval_infix(expr)?;
        self.push(v);
        Ok(())
    }

    /// `U` (`calc-undo`): restore the stack to before the last mutation.
    pub fn undo(&mut self) -> bool {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.stack, prev));
            true
        } else {
            false
        }
    }

    /// `D` (`calc-redo`): reapply the last undone mutation.
    pub fn redo(&mut self) -> bool {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.stack, next));
            true
        } else {
            false
        }
    }

    /// Toggle between radians and degrees for the trig functions.
    pub fn toggle_angle(&mut self) {
        self.angle = match self.angle {
            Angle::Radians => Angle::Degrees,
            Angle::Degrees => Angle::Radians,
        };
    }
}

/// Factorial extended to reals via a small Lanczos gamma (Calc `!` accepts
/// non-integers). Exact for non-negative integers, `gamma(x+1)` otherwise.
fn factorial(x: f64) -> Result<f64, String> {
    if x < 0.0 && x.fract() == 0.0 {
        return Err("factorial of a negative integer".into());
    }
    if x >= 0.0 && x.fract() == 0.0 && x <= 170.0 {
        let mut acc = 1.0;
        let mut k = 2.0;
        while k <= x {
            acc *= k;
            k += 1.0;
        }
        return Ok(acc);
    }
    let g = gamma(x + 1.0);
    if g.is_finite() {
        Ok(g)
    } else {
        Err("factorial overflow".into())
    }
}

/// Lanczos approximation of the gamma function (g=7, n=9 coefficients).
fn gamma(x: f64) -> f64 {
    const G: f64 = 7.0;
    const C: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        // Reflection formula for x < 0.5.
        std::f64::consts::PI / ((std::f64::consts::PI * x).sin() * gamma(1.0 - x))
    } else {
        let x = x - 1.0;
        let mut a = C[0];
        let t = x + G + 0.5;
        for (i, &c) in C.iter().enumerate().skip(1) {
            a += c / (x + i as f64);
        }
        (2.0 * std::f64::consts::PI).sqrt() * t.powf(x + 0.5) * (-t).exp() * a
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infix_matches_precedence() {
        assert_eq!(eval_infix("2+3*4").unwrap(), 14.0);
        assert_eq!(eval_infix("(2+3)*4").unwrap(), 20.0);
        assert_eq!(eval_infix("-5 + 3").unwrap(), -2.0);
        assert_eq!(eval_infix("2^10").unwrap(), 1024.0);
        assert_eq!(eval_infix("10 / 4").unwrap(), 2.5);
        assert_eq!(eval_infix("10 % 3").unwrap(), 1.0);
        assert_eq!(eval_infix("(-2)^2").unwrap(), 4.0);
        assert_eq!(eval_infix("1.5e2").unwrap(), 150.0);
        assert!(eval_infix("2 +").is_err());
        assert!(eval_infix("1/0").is_err());
        assert!(eval_infix("2 3").is_err());
        assert!(eval_infix("(1+2").is_err());
    }

    #[test]
    fn infix_functions_and_constants() {
        assert_eq!(eval_infix("sqrt(16)").unwrap(), 4.0);
        assert_eq!(eval_infix("abs(-3)").unwrap(), 3.0);
        assert!((eval_infix("pi").unwrap() - std::f64::consts::PI).abs() < 1e-12);
        assert!((eval_infix("sin(0)").unwrap()).abs() < 1e-12);
        assert!(eval_infix("bogus(1)").is_err());
    }

    #[test]
    fn format_trims() {
        assert_eq!(format_value(14.0), "14");
        assert_eq!(format_value(2.5), "2.5");
        assert_eq!(format_value(7.0), "7");
    }

    #[test]
    fn rpn_arithmetic() {
        let mut c = Calc::new();
        c.push(2.0);
        c.push(3.0);
        c.binop(BinOp::Add).unwrap();
        assert_eq!(c.top(), Some(5.0));
        c.push(4.0);
        c.binop(BinOp::Mul).unwrap();
        assert_eq!(c.top(), Some(20.0));
        assert_eq!(c.stack().len(), 1);
    }

    #[test]
    fn rpn_subtraction_order() {
        // 2: 10  1: 3  -> 10 - 3 = 7 (level2 op level1)
        let mut c = Calc::new();
        c.push(10.0);
        c.push(3.0);
        c.binop(BinOp::Sub).unwrap();
        assert_eq!(c.top(), Some(7.0));
        c.push(2.0);
        c.binop(BinOp::Div).unwrap(); // 7 / 2
        assert_eq!(c.top(), Some(3.5));
    }

    #[test]
    fn rpn_stack_ops() {
        let mut c = Calc::new();
        c.push(1.0);
        c.push(2.0);
        c.push(3.0); // [1,2,3]
        c.roll_down(); // top sinks to bottom -> [3,1,2]
        assert_eq!(c.stack(), &[3.0, 1.0, 2.0]);
        c.roll_up(); // inverse -> [1,2,3]
        assert_eq!(c.stack(), &[1.0, 2.0, 3.0]);
        c.enter(); // dup top -> [1,2,3,3]
        assert_eq!(c.stack(), &[1.0, 2.0, 3.0, 3.0]);
        c.pop(); // -> [1,2,3]
        assert_eq!(c.stack(), &[1.0, 2.0, 3.0]);
    }

    #[test]
    fn rpn_undo_redo() {
        let mut c = Calc::new();
        c.push(5.0);
        c.push(7.0);
        c.binop(BinOp::Add).unwrap(); // [12]
        assert_eq!(c.top(), Some(12.0));
        assert!(c.undo()); // back to [5,7]
        assert_eq!(c.stack(), &[5.0, 7.0]);
        assert!(c.redo()); // forward to [12]
        assert_eq!(c.stack(), &[12.0]);
        // A fresh action clears the redo ring.
        assert!(c.undo());
        c.push(1.0);
        assert!(!c.redo());
    }

    #[test]
    fn rpn_unary_and_angle() {
        let mut c = Calc::new();
        c.push(9.0);
        c.unop(UnOp::Sqrt).unwrap();
        assert_eq!(c.top(), Some(3.0));
        c.unop(UnOp::Neg).unwrap();
        assert_eq!(c.top(), Some(-3.0));
        c.unop(UnOp::Abs).unwrap();
        assert_eq!(c.top(), Some(3.0));

        c.angle = Angle::Degrees;
        c.push(90.0);
        c.unop(UnOp::Sin).unwrap();
        assert!((c.top().unwrap() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn rpn_factorial() {
        let mut c = Calc::new();
        c.push(5.0);
        c.unop(UnOp::Factorial).unwrap();
        assert_eq!(c.top(), Some(120.0));
        c.push(0.0);
        c.unop(UnOp::Factorial).unwrap();
        assert_eq!(c.top(), Some(1.0));
        // gamma(0.5+1) = 0.5*sqrt(pi)
        c.push(0.5);
        c.unop(UnOp::Factorial).unwrap();
        assert!((c.top().unwrap() - 0.886_226_925_452_758).abs() < 1e-9);
    }

    #[test]
    fn rpn_errors_do_not_mutate() {
        let mut c = Calc::new();
        c.push(1.0);
        assert!(c.binop(BinOp::Add).is_err()); // underflow
        assert_eq!(c.stack(), &[1.0]);
        c.push(0.0);
        assert!(c.binop(BinOp::Div).is_err()); // 1/0
        assert_eq!(c.stack(), &[1.0, 0.0]);
    }

    #[test]
    fn algebraic_entry_pushes() {
        let mut c = Calc::new();
        c.algebraic("2+3*4").unwrap();
        assert_eq!(c.top(), Some(14.0));
    }
}
