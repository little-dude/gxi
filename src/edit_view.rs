use cairo::Context;
use cairo::enums::HintStyle;
use gdk::*;
use gdk::enums::key;
use gtk::{
    *,
    self,
};
use rpc::{Core, self};
use serde_json::Value;
use std::cell::RefCell;
use std::cmp::{max, min};
use std::rc::Rc;
use std::u32;

use main_win::MainState;
use linecache::LineCache;
use theme::set_source_color;

pub struct EditView {
    core: Rc<RefCell<Core>>,
    main_state: Rc<RefCell<MainState>>,
    pub view_id: String,
    pub file_name: Option<String>,
    pub pristine: bool,
    pub da: DrawingArea,
    pub root_widget: gtk::Box,
    pub tab_widget: gtk::Box,
    pub label: Label,
    pub close_button: Button,
    hscrollbar: Scrollbar,
    vscrollbar: Scrollbar,
    line_cache: LineCache,
    font_height: f64,
    font_width: f64,
    font_ascent: f64,
    font_descent: f64,
}

impl EditView {
    pub fn new(main_state: Rc<RefCell<MainState>>, core: Rc<RefCell<Core>>, file_name: Option<String>, view_id: String) -> Rc<RefCell<EditView>> {
        let da = DrawingArea::new();
        let hscrollbar = Scrollbar::new(Orientation::Horizontal, None);
        let vscrollbar = Scrollbar::new(Orientation::Vertical, None);

        da.set_events(EventMask::BUTTON_PRESS_MASK.bits() as i32
            | EventMask::BUTTON_MOTION_MASK.bits() as i32
            | EventMask::SCROLL_MASK.bits() as i32
        );
        debug!("events={:?}", da.get_events());
        da.set_can_focus(true);

        let hbox = Box::new(Orientation::Horizontal, 0);
        let vbox = Box::new(Orientation::Vertical, 0);
        hbox.pack_start(&vbox, true, true, 0);
        hbox.pack_start(&vscrollbar, false, false, 0);
        vbox.pack_start(&da, true, true, 0);
        vbox.pack_start(&hscrollbar, false, false, 0);
        hbox.show_all();

        // Make the widgets for the tab
        let tab_hbox = gtk::Box::new(Orientation::Horizontal, 5);
        let label = Label::new(Some("blah"));
        tab_hbox.add(&label);
        let close_button = Button::new_from_icon_name("window-close", 0);
        tab_hbox.add(&close_button);
        tab_hbox.show_all();

        let edit_view = Rc::new(RefCell::new(EditView {
            core: core.clone(),
            main_state: main_state.clone(),
            file_name,
            pristine: true,
            view_id: view_id.clone(),
            da: da.clone(),
            root_widget: hbox.clone(),
            tab_widget: tab_hbox.clone(),
            label: label.clone(),
            close_button: close_button.clone(),
            hscrollbar: hscrollbar.clone(),
            vscrollbar: vscrollbar.clone(),
            line_cache: LineCache::new(),
            font_height: 1.0,
            font_width: 1.0,
            font_ascent: 1.0,
            font_descent: 1.0,
        }));

        edit_view.borrow_mut().update_title();

        da.connect_button_press_event(clone!(edit_view => move |_,eb| {
            edit_view.borrow().handle_button_press(eb)
        }));

        da.connect_draw(clone!(edit_view => move |_,ctx| {
            edit_view.borrow_mut().handle_draw(&ctx)
        }));

        da.connect_key_press_event(clone!(edit_view => move |_, ek| {
            edit_view.borrow_mut().handle_key_press_event(ek)
        }));

        da.connect_motion_notify_event(clone!(edit_view => move |_,em| {
            edit_view.borrow_mut().handle_drag(em)
        }));

        da.connect_realize(|w|{
            // Set the text cursor
            DisplayManager::get().get_default_display()
                .map(|disp| {
                    let cur = Cursor::new_for_display(&disp, CursorType::Xterm);
                    w.get_window().map(|win| win.set_cursor(&cur));
            });
            w.grab_focus();
        });

        da.connect_scroll_event(clone!(edit_view => move |_,es| {
            edit_view.borrow_mut().handle_scroll(es)
        }));

        da.connect_size_allocate(clone!(edit_view => move |_,alloc| {
            debug!("Size changed to w={} h={}", alloc.width, alloc.height);
            edit_view.borrow_mut().da_size_allocate(alloc.width, alloc.height);
        }));

        vscrollbar.connect_change_value(clone!(edit_view => move |_,_,value| {
            edit_view.borrow_mut().vscrollbar_change_value(value)
        }));

        use std::ffi::CString;
        use fontconfig::fontconfig;
        unsafe {
            let ret = fontconfig::FcConfigAppFontAddDir(
                fontconfig::FcConfigGetCurrent(),
                CString::new("fonts").unwrap().as_ptr() as *const u8,
            );
            debug!("fc ret = {}", ret);
        }

        edit_view
    }
}


fn convert_gtk_modifier(mt: ModifierType) -> u32 {
    let mut ret = 0;
    if mt.contains(ModifierType::SHIFT_MASK) { ret |= rpc::XI_SHIFT_KEY_MASK; }
    if mt.contains(ModifierType::CONTROL_MASK) { ret |= rpc::XI_CONTROL_KEY_MASK; }
    if mt.contains(ModifierType::MOD1_MASK) { ret |= rpc::XI_ALT_KEY_MASK; }    
    ret
}

impl EditView {
    pub fn set_file(&mut self, file_name: &str) {
        self.file_name = Some(file_name.to_string());
        self.update_title();
    }

    fn update_title(&self) {
        let title = match self.file_name {
            Some(ref f) => {
                f.split(::std::path::MAIN_SEPARATOR).last().unwrap_or("Untitled").to_string()
            }
            None => "Untitled".to_string()
        };

        let mut full_title = String::new();
        if !self.pristine {
            full_title.push('*');
        }
        full_title.push_str(&title);

        trace!("setting title to {}", full_title);
        self.label.set_text(&full_title);
    }

    pub fn update(&mut self, params: &Value) {
        let update = &params["update"];
        self.line_cache.apply_update(update);


        // let (text_width, text_height) = self.get_text_size();
        // debug!("{}{}", text_width, text_height);
        // let (lwidth, lheight) = self.layout.get_size();
        // debug!("{}{}", lwidth, lheight);
        // if (lwidth as f64) < text_width || (lheight as f64) < text_height {
        //     error!("hi");
        //     self.layout.set_size(text_width as u32 * 2, text_height as u32 * 2);
        // }

        // update scrollbars to the new text width and height
        let (text_width, text_height) = self.get_text_size();
        let vadj = self.vscrollbar.get_adjustment();
        vadj.set_lower(0f64);
        vadj.set_upper(text_height as f64);
        if vadj.get_value() + vadj.get_page_size() > vadj.get_upper() {
            vadj.set_value(vadj.get_upper() - vadj.get_page_size())
        }

        let hadj = self.hscrollbar.get_adjustment();
        hadj.set_lower(0f64);
        hadj.set_upper(text_width as f64);
        if hadj.get_value() + hadj.get_page_size() > hadj.get_upper() {
            hadj.set_value(hadj.get_upper() - hadj.get_page_size())
        }

        if let Some(pristine) = update["pristine"].as_bool() {
            if self.pristine != pristine {
                self.pristine = pristine;
                self.update_title();
            }
        }

        // self.change_scrollbar_visibility();

        self.da.queue_draw();
    }

    fn change_scrollbar_visibility(&self) {
        let vadj = self.vscrollbar.get_adjustment();
        let hadj = self.hscrollbar.get_adjustment();

        if vadj.get_value() <= vadj.get_lower()
            && vadj.get_value() + vadj.get_page_size() >= vadj.get_upper() {
            self.vscrollbar.hide();
        } else {
            self.vscrollbar.show();
        }

        if hadj.get_value() <= hadj.get_lower()
            && hadj.get_value() + hadj.get_page_size() >= hadj.get_upper() {
            self.hscrollbar.hide();
        } else {
            debug!("SHOWING HSCROLLBAR: {} {}-{} {}", hadj.get_value(), hadj.get_lower(), hadj.get_upper(), hadj.get_page_size());
            self.hscrollbar.show();
        }
    }

    pub fn da_px_to_cell(&self, x: f64, y: f64) -> (u64, u64) {
        // let first_line = (vadj.get_value() / font_extents.height) as usize;
        let x = x + self.hscrollbar.get_adjustment().get_value();
        let y = y + self.vscrollbar.get_adjustment().get_value();

        let mut y = y - self.font_descent;
        if y < 0.0 { y = 0.0; }
        ( (x / self.font_width + 0.5) as u64, (y / self.font_height) as u64)
    }

    fn da_size_allocate(&mut self, da_width: i32, da_height: i32) {
        debug!("DA SIZE ALLOCATE");
        let vadj = self.vscrollbar.get_adjustment();
        vadj.set_page_size(da_height as f64);
        let hadj = self.hscrollbar.get_adjustment();
        hadj.set_page_size(da_width as f64);

        self.update_visible_scroll_region();
    }

    fn vscrollbar_change_value(&mut self, value: f64) -> Inhibit {
        debug!("scroll changed value {}", value);

        self.update_visible_scroll_region();

        Inhibit(false)
    }

    fn update_visible_scroll_region(&self) {
        let da_height = self.da.get_allocated_height();
        let (_, first_line) = self.da_px_to_cell(0.0, 0.0);
        let (_, last_line) = self.da_px_to_cell(0.0, da_height as f64);
        let last_line = last_line + 1;

        debug!("update visible scroll region {} {}", first_line, last_line);
        self.core.borrow().scroll(&self.view_id, first_line, last_line);
    }

    fn get_text_size(&self) -> (f64, f64) {
        let da_width = self.da.get_allocated_width() as f64;
        let da_height = self.da.get_allocated_height() as f64;
        let num_lines = self.line_cache.height();

        let all_text_height = num_lines as f64 * self.font_height + self.font_descent;
        let height = if da_height > all_text_height {
            da_height
        } else {
            all_text_height
        };

        // let all_text_width = self.line_cache.width() as f64 * self.font_width;
        // TODO FIX 100
        // TODO FIX 100
        // TODO FIX 100
        // TODO FIX 100
        let all_text_width = 100 as f64 * self.font_width;
        let width = if da_width > all_text_width {
            da_width
        } else {
            all_text_width
        };
        (width, height)
    }

    pub fn handle_draw(&mut self, cr: &Context) -> Inhibit {
        // let foreground = self.main_state.borrow().theme.foreground;
        let theme = &self.main_state.borrow().theme;

        let da_width = self.da.get_allocated_width();
        let da_height = self.da.get_allocated_height();

        //debug!("Drawing");
        // cr.select_font_face("Mono", ::cairo::enums::FontSlant::Normal, ::cairo::enums::FontWeight::Normal);
        // let mut font_options = cr.get_font_options();
        // debug!("font options: {:?} {:?} {:?}", font_options, font_options.get_antialias(), font_options.get_hint_style());
        // font_options.set_hint_style(HintStyle::Full);
        cr.select_font_face("Inconsolata", ::cairo::enums::FontSlant::Normal, ::cairo::enums::FontWeight::Normal);
        cr.set_font_size(16.0);
        let font_extents = cr.font_extents();

        self.font_height = font_extents.height;
        self.font_width = font_extents.max_x_advance;
        self.font_ascent = font_extents.ascent;
        self.font_descent = font_extents.descent;

        // let (text_width, text_height) = self.get_text_size();
        let num_lines = self.line_cache.height();

        let vadj = self.vscrollbar.get_adjustment();
        let hadj = self.hscrollbar.get_adjustment();
        trace!("drawing.  vadj={}, {}", vadj.get_value(), vadj.get_upper());

        let first_line = (vadj.get_value() / font_extents.height) as u64;
        let last_line = ((vadj.get_value() + da_height as f64) / font_extents.height) as u64 + 1;
        let last_line = min(last_line, num_lines);

        // debug!("line_cache {} {} {}", self.line_cache.n_invalid_before, self.line_cache.lines.len(), self.line_cache.n_invalid_after);
        // let missing = self.line_cache.get_missing(first_line, last_line);

        // Find missing lines
        let mut found_missing = false;
        for i in first_line..last_line {
            if self.line_cache.get_line(i).is_none() {
                error!("missing line {}", i);
                found_missing = true;
            }
        }

        // We've already missed our chance to draw these lines, but we need to request them for the
        // next frame.  This needs to be improved to prevent flashing.
        if found_missing {
            error!("didn't have some lines, requesting, lines {}-{}", first_line, last_line);
            self.core.borrow().request_lines(&self.view_id, first_line as u64, last_line as u64);
        }

        // Draw background
        set_source_color(cr, theme.background);
        cr.rectangle(0.0, 0.0, da_width as f64, da_height as f64);
        cr.fill();


        // Highlight cursor lines
        // for i in first_line..last_line {
        //     cr.set_source_rgba(0.8, 0.8, 0.8, 1.0);
        //     if let Some(line) = self.line_cache.get_line(i) {

        //         if !line.cursor().is_empty() {
        //             cr.set_source_rgba(0.23, 0.23, 0.23, 1.0);
        //             cr.rectangle(0f64,
        //                 font_extents.height*((i+1) as f64) - font_extents.ascent - vadj.get_value(),
        //                 da_width as f64,
        //                 font_extents.ascent + font_extents.descent);
        //             cr.fill();
        //         }
        //     }
        // }

        const CURSOR_WIDTH: f64 = 2.0;

        let main_state = self.main_state.borrow();

        for i in first_line..last_line {
            // Keep track of the starting x position

            if let Some(line) = self.line_cache.get_line(i) {

                let line_view = if line.text().ends_with('\n') {
                    &line.text()[0..line.text().len()-1]
                } else {
                    &line.text()
                };

                // Draw the whole line, no styles
                cr.move_to(-hadj.get_value(),
                    font_extents.height*((i+1) as f64) - vadj.get_value()
                );
                set_source_color(cr, theme.foreground);
                cr.show_text(line_view);


                // KLUDGE  until we have real font and style handling, for now, we just draw the
                // styles in reverse order, on top of each other, overwriting the previous styles
                struct AbsStyle {
                    id: usize,
                    start: i64,
                    len: i64,
                }
                let mut abs_styles = Vec::new();

                let mut ix = 0;
                for style in &line.styles {
                    abs_styles.push(AbsStyle{
                        id: style.id,
                        start: ix + style.start,
                        len: style.len as i64,
                    });
                    ix += style.start + style.len as i64;
                }
                abs_styles.reverse();

                for style in &abs_styles {
                    cr.save();

                    // Draw background, create clip
                    if let Some(bg_color) = main_state.styles.get(style.id).and_then(|s| s.bg_color) {
                        set_source_color(cr, ::theme::Color::make_u32_argb(bg_color));
                    } else if style.id == 0 {
                        set_source_color(cr, theme.selection);
                    } else {
                        set_source_color(cr, theme.background);
                    }
                    // set_source_color(cr, ::theme::Color::make_u8((((style.id>>2) & 1)*255) as u8, (((style.id>>1) & 1)*255) as u8, (((style.id>>0) & 1)*255) as u8, 255));

                    cr.rectangle(font_extents.max_x_advance* (style.start as f64) - hadj.get_value(),
                        font_extents.height*((i+1) as f64) - font_extents.ascent - vadj.get_value(),
                        font_extents.max_x_advance* (style.len as f64),
                        font_extents.ascent + font_extents.descent);
                    cr.clip_preserve();
                    cr.fill();

                    
                    // Draw the whole line, clipped
                    cr.move_to(-hadj.get_value(),
                        font_extents.height*((i+1) as f64) - vadj.get_value()
                    );
                    // set_source_color(cr, theme.foreground); // TODO styled def color
                    if let Some(fg_color) = main_state.styles.get(style.id).and_then(|s| s.fg_color) {
                        set_source_color(cr, ::theme::Color::make_u32_argb(fg_color));
                    } else if style.id == 0 {
                        set_source_color(cr, theme.selection_foreground);
                    } else {
                        set_source_color(cr, theme.foreground);
                    }
                    cr.show_text(line_view);

                    cr.restore();
                }

                // Draw the cursor
                set_source_color(cr, theme.caret);
                for c in line.cursor() {
                    cr.rectangle(font_extents.max_x_advance* (*c as f64) - hadj.get_value(),
                        font_extents.height*((i+1) as f64) - font_extents.ascent - vadj.get_value(),
                        CURSOR_WIDTH,
                        font_extents.ascent + font_extents.descent);
                    cr.fill();
                }

            }
        }

        Inhibit(false)
    }

    pub fn scroll_to(&mut self, line: u64, col: u64) {
        {
            let cur_top = self.font_height*((line+1) as f64) - self.font_ascent;
            let cur_bottom = cur_top + self.font_ascent + self.font_descent;
            let vadj = self.vscrollbar.get_adjustment();
            if cur_top < vadj.get_value() {
                vadj.set_value(cur_top);
            } else if cur_bottom > vadj.get_value() + vadj.get_page_size() && vadj.get_page_size() != 0.0 {
                vadj.set_value(cur_bottom - vadj.get_page_size());
            }
        }

        {
            let cur_left = self.font_width*(col as f64) - self.font_ascent;
            let cur_right = cur_left + self.font_width*2.0;
            let hadj = self.hscrollbar.get_adjustment();
            if cur_left < hadj.get_value() {
                hadj.set_value(cur_left);
            } else if cur_right > hadj.get_value() + hadj.get_page_size() && hadj.get_page_size() != 0.0 {
                hadj.set_value(cur_right - hadj.get_page_size());
            }
        }
    }

    pub fn handle_button_press(&self, eb: &EventButton) -> Inhibit {
        self.da.grab_focus();

        let (x,y) = eb.get_position();
        let (col, line) = self.da_px_to_cell(x, y);

        match eb.get_button() {
            1 => {
                if eb.get_state().contains(ModifierType::SHIFT_MASK) {
                    self.core.borrow().gesture_range_select(&self.view_id, line, col);
                } else if eb.get_event_type() == EventType::DoubleButtonPress {
                    self.core.borrow().gesture_word_select(&self.view_id, line, col);
                } else if eb.get_event_type() == EventType::TripleButtonPress {
                    self.core.borrow().gesture_line_select(&self.view_id, line, col);
                } else {
                    self.core.borrow().gesture_point_select(&self.view_id, line, col);
                }
            },
            2 => {
                self.do_paste_primary(&self.view_id, line, col);
            },
            _ => {},
        }
        Inhibit(false)
    }

    pub fn handle_drag(&mut self, em: &EventMotion) -> Inhibit {
        let (x,y) = em.get_position();
        let (col, line) = self.da_px_to_cell(x, y);
        self.core.borrow().drag(&self.view_id, line, col, convert_gtk_modifier(em.get_state()));
        Inhibit(false)
    }

    pub fn handle_scroll(&mut self, es: &EventScroll) -> Inhibit {
        debug!("scroll {:?}", es);
        self.da.grab_focus();
        let amt = self.font_height * 3.0;

        let vadj = self.vscrollbar.get_adjustment();
        let hadj = self.hscrollbar.get_adjustment();
        match es.get_direction() {
            ScrollDirection::Up => vadj.set_value(vadj.get_value() - amt),
            ScrollDirection::Down => vadj.set_value(vadj.get_value() + amt),
            ScrollDirection::Left => hadj.set_value(hadj.get_value() - amt),
            ScrollDirection::Right => hadj.set_value(hadj.get_value() + amt),
            _ => {},
        }

        self.update_visible_scroll_region();

        Inhibit(false)
    }

    fn handle_key_press_event(&mut self, ek: &EventKey) -> Inhibit {
        debug!("key press {:?}", ek);
        debug!("key press keyval={:?}, state={:?}, length={:?} group={:?} uc={:?}",
            ek.get_keyval(), ek.get_state(), ek.get_length(), ek.get_group(),
            ::gdk::keyval_to_unicode(ek.get_keyval())
        );
        let view_id = &self.view_id;
        let ch = ::gdk::keyval_to_unicode(ek.get_keyval());

        let alt = ek.get_state().contains(ModifierType::MOD1_MASK);
        let ctrl = ek.get_state().contains(ModifierType::CONTROL_MASK);
        let meta = ek.get_state().contains(ModifierType::META_MASK);
        let shift = ek.get_state().contains(ModifierType::SHIFT_MASK);
        let norm = !alt && !ctrl && !meta;

        match ek.get_keyval() {
            key::Delete if norm => self.core.borrow().delete_forward(view_id),
            key::BackSpace if norm => self.core.borrow().delete_backward(view_id),
            key::Return | key::KP_Enter => {
                self.core.borrow().insert_newline(&view_id);
            },
            key::Tab if norm && !shift => self.core.borrow().insert_tab(view_id),
            key::Up if norm && !shift  => self.core.borrow().move_up(view_id),
            key::Down if norm && !shift  => self.core.borrow().move_down(view_id),
            key::Left if norm && !shift => self.core.borrow().move_left(view_id),
            key::Right if norm && !shift  => self.core.borrow().move_right(view_id),
            key::Up if norm && shift => {
                self.core.borrow().move_up_and_modify_selection(view_id);
            },
            key::Down if norm && shift => {
                self.core.borrow().move_down_and_modify_selection(view_id);
            },
            key::Left if norm && shift => {
                self.core.borrow().move_left_and_modify_selection(view_id);
            },
            key::Right if norm && shift => {
                self.core.borrow().move_right_and_modify_selection(view_id);
            },
            key::Left if ctrl && !shift => {
                self.core.borrow().move_word_left(view_id);
            },
            key::Right if ctrl && !shift => {
                self.core.borrow().move_word_right(view_id);
            },
            key::Left if ctrl && shift => {
                self.core.borrow().move_word_left_and_modify_selection(view_id);
            },
            key::Right if ctrl && shift => {
                self.core.borrow().move_word_right_and_modify_selection(view_id);
            },
            key::Home if norm && !shift => {
                self.core.borrow().move_to_left_end_of_line(view_id);
            }
            key::End if norm && !shift => {
                self.core.borrow().move_to_right_end_of_line(view_id);
            }
            key::Home if norm && shift => {
                self.core.borrow().move_to_left_end_of_line_and_modify_selection(view_id);
            }
            key::End if norm && shift => {
                self.core.borrow().move_to_right_end_of_line_and_modify_selection(view_id);
            }
            key::Home if ctrl && !shift => {
                self.core.borrow().move_to_beginning_of_document(view_id);
            }
            key::End if ctrl && !shift => {
                self.core.borrow().move_to_end_of_document(view_id);
            }
            key::Home if ctrl && shift => {
                self.core.borrow().move_to_beginning_of_document_and_modify_selection(view_id);
            }
            key::End if ctrl && shift => {
                self.core.borrow().move_to_end_of_document_and_modify_selection(view_id);
            }
            key::Page_Up if norm && !shift => {
                self.core.borrow().page_up(view_id);
            }
            key::Page_Down if norm && !shift => {
                self.core.borrow().page_down(view_id);
            }
            key::Page_Up if norm && shift => {
                self.core.borrow().page_up_and_modify_selection(view_id);
            }
            key::Page_Down if norm && shift => {
                self.core.borrow().page_down_and_modify_selection(view_id);
            }
            _ => {
                if let Some(ch) = ch {
                    match ch {
                        'a' if ctrl => {
                            self.core.borrow().select_all(view_id);
                        },
                        'c' if ctrl => {
                            self.do_copy(view_id);
                        },
                        'v' if ctrl => {
                            self.do_paste(view_id);
                        },
                        't' if ctrl => {
                            // TODO new tab
                        },
                        'x' if ctrl => {
                            self.do_cut(view_id);
                        },
                        'z' if ctrl => {
                            self.core.borrow().undo(view_id);
                        },
                        'Z' if ctrl && shift => {
                            self.core.borrow().redo(view_id);
                        },
                        c if (norm) && c >= '\u{0020}' => {
                            debug!("inserting key");
                            self.core.borrow().insert(view_id, &c.to_string());
                        }
                        _ => {
                            debug!("unhandled key: {:?}", ch);
                        },
                    }
                }
            },
        };
        Inhibit(true)
    }

    fn do_cut(&self, view_id: &str) {
        if let Some(text) = self.core.borrow_mut().cut(view_id) {
            Clipboard::get(&SELECTION_CLIPBOARD).set_text(&text);
        }
    }

    fn do_copy(&self, view_id: &str) {
        if let Some(text) = self.core.borrow_mut().copy(view_id) {
            Clipboard::get(&SELECTION_CLIPBOARD).set_text(&text);
        }
    }

    fn do_paste(&self, view_id: &str) {
        // if let Some(text) = Clipboard::get(&SELECTION_CLIPBOARD).wait_for_text() {
        //     self.core.borrow().insert(view_id, &text);
        // }
        use clipboard::ClipboardRequest;
        let view_id2 = view_id.to_string().clone();
        let core = self.core.clone();
        Clipboard::get(&SELECTION_CLIPBOARD).request_text(move |_, text|{
            core.borrow().insert(&view_id2, &text);
        });
    }

    fn do_paste_primary(&self, view_id: &str, line: u64, col: u64) {
        // if let Some(text) = Clipboard::get(&SELECTION_PRIMARY).wait_for_text() {
        //     self.core.borrow().insert(view_id, &text);
        // }
        use clipboard::ClipboardRequest;
        let view_id2 = view_id.to_string().clone();
        let core = self.core.clone();
        Clipboard::get(&SELECTION_PRIMARY).request_text(move |_, text|{
            core.borrow().gesture_point_select(&view_id2, line, col);
            core.borrow().insert(&view_id2, &text);
        });
    }
}