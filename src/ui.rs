use crate::{context::Context, delete::Deleted, scan::ConversionArgs};
use ::anyhow::{Context as _, Result};
use ::futures::Future;
use ::ratatui::{
	Terminal,
	backend::{Backend, CrosstermBackend},
};
use ::std::{
	borrow::Cow, cell::RefCell, collections::HashMap, io, mem, rc::Rc, sync::Arc, time::Duration,
};
use ::tokio::{task, time::interval};

pub const UPDATE_INTERVAL_MILLIS: u64 = 100;

#[derive(Debug)]
pub enum Msg {
	Exit,
	TaskStart { id: usize, args: ConversionArgs },
	TaskEnd { id: usize },
	TaskProgress { id: usize, ratio: f64 },
	TaskError { id: usize },
}

#[derive(Debug, Clone)]
pub struct MsgQueue {
	inner: Rc<RefCell<Vec<Msg>>>,
}

impl MsgQueue {
	fn new() -> MsgQueue {
		MsgQueue {
			inner: Rc::new(RefCell::new(Vec::new())),
		}
	}

	pub fn push(&self, msg: Msg) {
		self.inner.borrow_mut().push(msg);
	}

	fn swap_inner(&self, other: &mut Vec<Msg>) {
		let mut inner = self.inner.borrow_mut();
		mem::swap(&mut *inner, other)
	}
}

struct State {
	terminal: Terminal<CrosstermBackend<io::Stdout>>,
	ctx: Arc<Context>,
	task_len: usize,
	ended_tasks: usize,
	running_tasks: HashMap<usize, Task>,
	has_rendered: bool,
	has_errored: bool,
	deleted: Option<Deleted>,
}

impl State {
	fn new(
		ctx: Arc<Context>,
		task_len: usize,
		deleted: Option<Deleted>,
		has_errored: bool,
	) -> Result<State> {
		let terminal = Terminal::new(CrosstermBackend::new(io::stdout()))
			.context("Unable to create ui terminal")?;

		Ok(State {
			terminal,
			ctx,
			task_len,
			ended_tasks: 0,
			running_tasks: HashMap::new(),
			has_rendered: false,
			has_errored: has_errored || deleted.as_ref().is_some_and(|deleted| deleted.failed > 0),
			deleted,
		})
	}

	fn process_msg(&mut self, msg: Msg) -> Result<bool> {
		match msg {
			Msg::Exit => return Ok(false),
			Msg::TaskStart { id, args } => {
				self.running_tasks.insert(
					id,
					Task {
						id,
						ratio: None,
						args,
					},
				);
			}
			Msg::TaskEnd { id } => {
				self.running_tasks
					.remove(&id)
					.context("Unable to remove finished task; could't find task")?;
				self.ended_tasks += 1;
			}
			Msg::TaskProgress { id, ratio } => {
				let task = self
					.running_tasks
					.get_mut(&id)
					.context("Unable to update task progress; could't find task")?;
				task.ratio = Some(ratio);
			}
			Msg::TaskError { id } => {
				self.running_tasks
					.remove(&id)
					.context("Unable to remove errored task; could't find task")?;
				self.ended_tasks += 1;
				self.has_errored = true;
			}
		}

		Ok(true)
	}

	fn render(&mut self) -> Result<()> {
		use ::ratatui::{
			layout::{Constraint, Direction, Layout, Rect},
			style::{Color, Modifier, Style},
			text::{Line, Text},
			widgets::{Block, Borders, Gauge, Paragraph},
		};

		let task_len = self.task_len;

		if task_len == 0 && self.deleted.is_none() && !self.has_errored {
			return Ok(());
		}

		let tasks_ended = self.ended_tasks;

		let mut running_tasks: Vec<_> = self.running_tasks.values().cloned().collect();

		running_tasks.sort_by_key(|task| task.id);

		if !self.has_rendered {
			// `Terminal::clear` would query the cursor position, which needs a responding terminal
			self.terminal
				.backend_mut()
				.clear()
				.context("Clearing ui failed")?;
			self.has_rendered = true;
		}

		let logged_to = format!(" and were logged to {}", self.ctx.log.path().display());

		let mut status = Vec::new();

		if let Some(deleted) = &self.deleted {
			let text = match (deleted.files, deleted.force) {
				(0, _) => Cow::Borrowed("No files to delete"),
				(files, false) => Cow::Owned(format!(
					"{files} file(s) would be deleted{logged_to}, use --force to delete them"
				)),
				(files, true) => Cow::Owned(format!("{files} file(s) were deleted{logged_to}")),
			};
			status.push(Line::styled(
				text,
				Style::default().fg(Color::Yellow).bg(Color::Black),
			));
		}

		if self.has_errored {
			status.push(Line::styled(
				format!("Error(s) occurred{logged_to}"),
				Style::default()
					.fg(Color::Red)
					.bg(Color::Black)
					.add_modifier(Modifier::BOLD),
			));
		}

		let status_height = status.len() as u16;

		self.terminal
			.draw(|f| {
				let chunks = Layout::default()
					.direction(Direction::Vertical)
					.margin(1)
					.constraints([Constraint::Percentage(90), Constraint::Percentage(10)].as_ref())
					.split(f.area());

				let mut task_rect = chunks[0];

				if status_height > 0 {
					task_rect.height = task_rect.height.saturating_sub(status_height + 1);
				}

				for (row, task) in running_tasks
					.into_iter()
					.take(task_rect.height as usize / 2)
					.enumerate()
				{
					f.render_widget(
						Gauge::default()
							.label(task.args.rel_from_path.to_string_lossy().as_ref())
							.gauge_style(
								Style::default()
									.fg(Color::White)
									.bg(Color::Black)
									.add_modifier(Modifier::ITALIC),
							)
							.ratio(task.ratio.unwrap_or(0.0)),
						Rect::new(
							task_rect.x,
							task_rect.y + row as u16 * 2,
							task_rect.width,
							1,
						),
					);
				}

				if status_height > 0 {
					f.render_widget(
						Paragraph::new(Text::from(status)),
						Rect::new(
							task_rect.x,
							task_rect.y + task_rect.height + 1,
							task_rect.width,
							status_height,
						),
					);
				}

				f.render_widget(
					Gauge::default()
						.block(
							Block::default()
								.borders(Borders::ALL)
								.title("Overall Progress"),
						)
						.gauge_style(
							Style::default()
								.fg(Color::White)
								.bg(Color::Black)
								.add_modifier(Modifier::ITALIC),
						)
						.ratio(if task_len == 0 {
							1.0
						} else {
							tasks_ended as f64 / task_len as f64
						}),
					chunks[1],
				);
			})
			.context("Rendering ui failed")?;

		Ok(())
	}
}

#[derive(Debug, Clone)]
struct Task {
	id: usize,
	ratio: Option<f64>,
	args: ConversionArgs,
}

pub fn init(
	ctx: Arc<Context>,
	task_len: usize,
	deleted: Option<Deleted>,
	// errors logged before the ui started
	has_errored: bool,
) -> (MsgQueue, impl Future<Output = Result<()>>) {
	let queue = MsgQueue::new();

	let queue_clone = queue.clone();
	let fut = async move {
		let mut interval = interval(Duration::from_millis(UPDATE_INTERVAL_MILLIS));
		let mut wrapped = Some((Vec::new(), State::new(ctx, task_len, deleted, has_errored)?));

		loop {
			interval.tick().await;

			let (mut current_queue, mut state) = wrapped.take().context("`wrapped` is None")?;

			queue_clone.swap_inner(&mut current_queue);

			let render_res = task::spawn_blocking(move || -> Result<_> {
				let mut exit = false;
				for msg in current_queue.drain(..) {
					if !state.process_msg(msg)? {
						exit = true;
					}
				}

				state.render()?;

				if exit {
					Ok(None)
				} else {
					Ok(Some((current_queue, state)))
				}
			})
			.await
			.context("Ui update task failed")?
			.context("Ui update failed")?;

			match render_res {
				Some(s) => wrapped = Some(s),
				None => break,
			}
		}

		Result::<_>::Ok(())
	};

	(queue, fut)
}
