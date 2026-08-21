use crate::model::{display_bytes, DbSnapshot, Selection};
use crate::screen::{clip_text_cells, text_cell_width};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    pub fn right(self) -> i32 {
        self.x + self.width as i32 - 1
    }

    pub fn bottom(self) -> i32 {
        self.y + self.height as i32 - 1
    }

    pub fn center_y(self) -> i32 {
        self.y + self.height as i32 / 2
    }

    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x && x <= self.right() && y >= self.y && y <= self.bottom()
    }

    pub fn translated(self, dx: i32, dy: i32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            ..self
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Database,
    Table,
    Row,
}

#[derive(Clone, Debug)]
pub struct LayoutNode {
    pub selection: Selection,
    pub kind: NodeKind,
    pub rect: Rect,
    pub title: String,
    pub detail: String,
}

#[derive(Clone, Copy, Debug)]
pub struct LayoutEdge {
    pub parent: usize,
    pub child: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub min_x: i32,
    pub min_y: i32,
    pub max_x: i32,
    pub max_y: i32,
}

impl Bounds {
    pub fn center_x(self) -> i32 {
        (self.min_x + self.max_x) / 2
    }

    pub fn center_y(self) -> i32 {
        (self.min_y + self.max_y) / 2
    }
}

#[derive(Clone, Debug)]
pub struct GraphLayout {
    pub nodes: Vec<LayoutNode>,
    pub edges: Vec<LayoutEdge>,
    pub bounds: Bounds,
}

impl GraphLayout {
    pub fn build(snapshot: &DbSnapshot, show_values: bool, spacing: usize) -> Self {
        let spacing = spacing.min(3) as i32;
        let table_x = 28 + spacing * 3;
        let row_x = 56 + spacing * 6;
        let row_step = 4 + spacing;
        let table_gap = 3 + spacing * 2;

        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        nodes.push(LayoutNode {
            selection: Selection::Database,
            kind: NodeKind::Database,
            rect: Rect {
                x: 0,
                y: 0,
                width: 22,
                height: 3,
            },
            title: "RAM database".to_owned(),
            detail: format!(
                "{} tables ・ {} rows",
                snapshot.tables.len(),
                snapshot.total_rows()
            ),
        });

        let mut cursor_y = 0i32;
        let mut table_node_indices = Vec::new();
        for table in &snapshot.tables {
            let first_row_y = cursor_y;
            let row_count = table.rows.len();
            let last_row_y = if row_count > 0 {
                first_row_y + (row_count.saturating_sub(1) as i32 * row_step)
            } else {
                first_row_y
            };
            let table_y = (first_row_y + last_row_y) / 2;
            let table_index = nodes.len();
            table_node_indices.push(table_index);
            nodes.push(LayoutNode {
                selection: Selection::Table {
                    table: table.name.clone(),
                },
                kind: NodeKind::Table,
                rect: Rect {
                    x: table_x,
                    y: table_y,
                    width: 22,
                    height: 3,
                },
                title: clip_text_cells(&table.name, 18),
                detail: if table.truncated {
                    format!("{}+ rows", table.rows.len())
                } else {
                    format!("{} rows", table.rows.len())
                },
            });
            edges.push(LayoutEdge {
                parent: 0,
                child: table_index,
            });

            for (row_index, row) in table.rows.iter().enumerate() {
                let key = display_bytes(&row.key);
                let value = display_bytes(&row.value);
                let node_index = nodes.len();
                nodes.push(LayoutNode {
                    selection: Selection::Row {
                        table: table.name.clone(),
                        key: row.key.clone(),
                    },
                    kind: NodeKind::Row,
                    rect: Rect {
                        x: row_x,
                        y: first_row_y + row_index as i32 * row_step,
                        width: 30,
                        height: 3,
                    },
                    title: clip_text_cells(&key, 26),
                    detail: if show_values {
                        clip_text_cells(&value, 26)
                    } else {
                        format!("{} bytes", row.value.len())
                    },
                });
                edges.push(LayoutEdge {
                    parent: table_index,
                    child: node_index,
                });
            }

            let occupied_rows = row_count.max(1) as i32;
            cursor_y += occupied_rows * row_step + table_gap;
        }

        let root_y = match (table_node_indices.first(), table_node_indices.last()) {
            (Some(first), Some(last)) => {
                (nodes[*first].rect.center_y() + nodes[*last].rect.center_y()) / 2
            }
            _ => 0,
        };
        for node in &mut nodes {
            node.rect.y -= root_y;
        }

        let bounds = compute_bounds(&nodes);
        Self {
            nodes,
            edges,
            bounds,
        }
    }

    pub fn node(&self, selection: &Selection) -> Option<&LayoutNode> {
        self.nodes.iter().find(|node| &node.selection == selection)
    }
}

fn compute_bounds(nodes: &[LayoutNode]) -> Bounds {
    let mut min_x = 0;
    let mut min_y = 0;
    let mut max_x = 0;
    let mut max_y = 0;
    for node in nodes {
        min_x = min_x.min(node.rect.x);
        min_y = min_y.min(node.rect.y);
        max_x = max_x.max(node.rect.right());
        max_y = max_y.max(node.rect.bottom());
    }
    Bounds {
        min_x,
        min_y,
        max_x,
        max_y,
    }
}

pub fn node_width_for_text(title: &str, detail: &str, minimum: u16, maximum: u16) -> u16 {
    let wanted = text_cell_width(title)
        .max(text_cell_width(detail))
        .saturating_add(4) as u16;
    wanted.clamp(minimum, maximum)
}

#[cfg(test)]
mod tests {
    use crate::model::{DbSnapshot, RowSnapshot, TableSnapshot};

    use super::GraphLayout;

    #[test]
    fn creates_database_table_and_row_nodes() {
        let snapshot = DbSnapshot {
            tables: vec![TableSnapshot {
                name: "x".to_owned(),
                rows: vec![RowSnapshot {
                    key: b"a".to_vec(),
                    value: b"b".to_vec(),
                }],
                truncated: false,
            }],
        };
        let graph = GraphLayout::build(&snapshot, true, 1);
        assert_eq!(graph.nodes.len(), 3);
        assert_eq!(graph.edges.len(), 2);
    }
}
