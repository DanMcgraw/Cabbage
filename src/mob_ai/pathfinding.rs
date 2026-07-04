use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashMap, VecDeque},
};
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};

pub const MAX_PATH_HEIGHT_DIFFERENCE: i32 = 8;
pub const MAX_PATH_GRID_VOLUME: usize = 65_536;
pub const CARDINAL_PATH_STEP_COST: u32 = 10;
pub const DIAGONAL_XZ_PATH_STEP_COST: u32 = 14;
pub const PATH_INTERVAL_HORIZONTAL_DISTANCE_MULTIPLIER_TICKS: i64 = 2;
pub const MIN_PATH_INTERVAL_TICKS: i64 = 5;

pub const NEIGHBOR_OFFSETS: [Vector3<i32>; 10] = [
    Vector3::new(1, 0, 0),
    Vector3::new(-1, 0, 0),
    Vector3::new(0, 1, 0),
    Vector3::new(0, -1, 0),
    Vector3::new(0, 0, 1),
    Vector3::new(0, 0, -1),
    Vector3::new(1, 0, 1),
    Vector3::new(1, 0, -1),
    Vector3::new(-1, 0, 1),
    Vector3::new(-1, 0, -1),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PathBounds {
    pub min: BlockPos,
    pub max: BlockPos,
}

impl PathBounds {
    pub fn between(a: BlockPos, b: BlockPos, outset: i32) -> Self {
        Self {
            min: BlockPos::new(
                a.0.x.min(b.0.x).saturating_sub(outset),
                a.0.y.min(b.0.y).saturating_sub(outset),
                a.0.z.min(b.0.z).saturating_sub(outset),
            ),
            max: BlockPos::new(
                a.0.x.max(b.0.x).saturating_add(outset),
                a.0.y.max(b.0.y).saturating_add(outset),
                a.0.z.max(b.0.z).saturating_add(outset),
            ),
        }
    }

    pub fn contains(&self, pos: BlockPos) -> bool {
        pos.0.x >= self.min.0.x
            && pos.0.x <= self.max.0.x
            && pos.0.y >= self.min.0.y
            && pos.0.y <= self.max.0.y
            && pos.0.z >= self.min.0.z
            && pos.0.z <= self.max.0.z
    }
}

pub struct BlockGrid {
    pub bounds: PathBounds,
    pub size_x: usize,
    pub size_y: usize,
    pub size_z: usize,
    pub closed: Vec<bool>,
}

impl BlockGrid {
    pub fn sample(mut bounds: PathBounds, mut is_closed: impl FnMut(BlockPos) -> bool) -> Option<Self> {
        normalize_bounds(&mut bounds);

        let size_x = axis_len(bounds.min.0.x, bounds.max.0.x)?;
        let size_y = axis_len(bounds.min.0.y, bounds.max.0.y)?;
        let size_z = axis_len(bounds.min.0.z, bounds.max.0.z)?;
        let volume = size_x.checked_mul(size_y)?.checked_mul(size_z)?;
        if volume > MAX_PATH_GRID_VOLUME {
            return None;
        }

        let mut closed = Vec::with_capacity(volume);

        for z in bounds.min.0.z..=bounds.max.0.z {
            for y in bounds.min.0.y..=bounds.max.0.y {
                for x in bounds.min.0.x..=bounds.max.0.x {
                    closed.push(is_closed(BlockPos::new(x, y, z)));
                }
            }
        }

        Some(Self {
            bounds,
            size_x,
            size_y,
            size_z,
            closed,
        })
    }

    pub fn is_open(&self, pos: BlockPos) -> bool {
        self.index(pos)
            .and_then(|index| self.closed.get(index))
            .is_some_and(|closed| !closed)
    }

    pub fn neighbors(&self, pos: BlockPos, forward_search: bool) -> Vec<(BlockPos, u32)> {
        NEIGHBOR_OFFSETS
            .iter()
            .filter_map(|offset| {
                let next = pos.offset(*offset);

                if !self.movement_is_allowed(pos, next, *offset, forward_search)
                    || !self.is_open(next)
                {
                    return None;
                }

                Some((next, movement_cost(*offset)))
            })
            .collect()
    }

    fn movement_is_allowed(
        &self,
        current: BlockPos,
        next: BlockPos,
        offset: Vector3<i32>,
        forward_search: bool,
    ) -> bool {
        if !self.bounds.contains(next) {
            return false;
        }

        if is_xz_diagonal_offset(offset)
            && (!self.is_open(current.add(offset.x, 0, 0))
                || !self.is_open(current.add(0, 0, offset.z)))
        {
            return false;
        }

        let delta_y = next.0.y - current.0.y;
        if forward_search {
            delta_y <= 1
        } else {
            delta_y >= -1
        }
    }

    fn index(&self, pos: BlockPos) -> Option<usize> {
        if !self.bounds.contains(pos) {
            return None;
        }

        let x = usize::try_from(pos.0.x - self.bounds.min.0.x).ok()?;
        let y = usize::try_from(pos.0.y - self.bounds.min.0.y).ok()?;
        let z = usize::try_from(pos.0.z - self.bounds.min.0.z).ok()?;

        debug_assert!(x < self.size_x);
        debug_assert!(y < self.size_y);
        debug_assert!(z < self.size_z);

        Some((z * self.size_y + y) * self.size_x + x)
    }
}

pub fn normalize_bounds(bounds: &mut PathBounds) {
    let min = BlockPos::new(
        bounds.min.0.x.min(bounds.max.0.x),
        bounds.min.0.y.min(bounds.max.0.y),
        bounds.min.0.z.min(bounds.max.0.z),
    );
    let max = BlockPos::new(
        bounds.min.0.x.max(bounds.max.0.x),
        bounds.min.0.y.max(bounds.max.0.y),
        bounds.min.0.z.max(bounds.max.0.z),
    );

    bounds.min = min;
    bounds.max = max;
}

pub fn axis_len(min: i32, max: i32) -> Option<usize> {
    let len = i64::from(max) - i64::from(min) + 1;
    usize::try_from(len).ok()
}

pub fn bidirectional_a_star(
    grid: &BlockGrid,
    start: BlockPos,
    goal: BlockPos,
) -> Option<Vec<BlockPos>> {
    if !grid.is_open(start) || !grid.is_open(goal) {
        return None;
    }

    if start == goal {
        return Some(vec![start]);
    }

    let mut forward = SearchData::new(start, goal);
    let mut backward = SearchData::new(goal, start);

    while !forward.open.is_empty() && !backward.open.is_empty() {
        let expand_forward = forward.peek_priority() <= backward.peek_priority();
        let meet = if expand_forward {
            expand_search(&mut forward, &backward, grid, goal, true)
        } else {
            expand_search(&mut backward, &forward, grid, start, false)
        };

        if let Some(meet) = meet {
            return Some(reconstruct_path(meet, &forward, &backward));
        }
    }

    None
}

fn expand_search(
    search: &mut SearchData,
    other_search: &SearchData,
    grid: &BlockGrid,
    target: BlockPos,
    forward_search: bool,
) -> Option<BlockPos> {
    let current = search.pop_current()?;

    if other_search.g_score.contains_key(&current) {
        return Some(current);
    }

    let current_g = search.g_score[&current];
    for (neighbor, step_cost) in grid.neighbors(current, forward_search) {
        let tentative_g = current_g.saturating_add(step_cost);
        if tentative_g >= *search.g_score.get(&neighbor).unwrap_or(&u32::MAX) {
            continue;
        }

        search.came_from.insert(neighbor, current);
        search.g_score.insert(neighbor, tentative_g);
        search.push(neighbor, tentative_g, path_heuristic(neighbor, target));

        if other_search.g_score.contains_key(&neighbor) {
            return Some(neighbor);
        }
    }

    None
}

fn reconstruct_path(meet: BlockPos, forward: &SearchData, backward: &SearchData) -> Vec<BlockPos> {
    let mut path_to_start = vec![meet];
    let mut current = meet;
    while let Some(previous) = forward.came_from.get(&current).copied() {
        path_to_start.push(previous);
        current = previous;
    }
    path_to_start.reverse();

    current = meet;
    while let Some(next) = backward.came_from.get(&current).copied() {
        path_to_start.push(next);
        current = next;
    }

    path_to_start
}

fn movement_cost(offset: Vector3<i32>) -> u32 {
    if is_xz_diagonal_offset(offset) {
        DIAGONAL_XZ_PATH_STEP_COST
    } else {
        CARDINAL_PATH_STEP_COST
    }
}

fn is_xz_diagonal_offset(offset: Vector3<i32>) -> bool {
    offset.x != 0 && offset.z != 0
}

fn path_heuristic(a: BlockPos, b: BlockPos) -> u32 {
    let dx = a.0.x.abs_diff(b.0.x);
    let dy = a.0.y.abs_diff(b.0.y);
    let dz = a.0.z.abs_diff(b.0.z);
    let diagonal = dx.min(dz);
    let straight = dx.max(dz) - diagonal;

    DIAGONAL_XZ_PATH_STEP_COST
        .saturating_mul(diagonal)
        .saturating_add(CARDINAL_PATH_STEP_COST.saturating_mul(straight))
        .saturating_add(CARDINAL_PATH_STEP_COST.saturating_mul(dy))
}

struct SearchData {
    open: BinaryHeap<QueueEntry>,
    g_score: HashMap<BlockPos, u32>,
    came_from: HashMap<BlockPos, BlockPos>,
}

impl SearchData {
    fn new(start: BlockPos, target: BlockPos) -> Self {
        let mut search = Self {
            open: BinaryHeap::new(),
            g_score: HashMap::new(),
            came_from: HashMap::new(),
        };
        search.g_score.insert(start, 0);
        search.push(start, 0, path_heuristic(start, target));
        search
    }

    fn push(&mut self, pos: BlockPos, g: u32, h: u32) {
        self.open.push(QueueEntry {
            pos,
            g,
            h,
            f: g.saturating_add(h),
        });
    }

    fn peek_priority(&self) -> u32 {
        self.open.peek().map_or(u32::MAX, |entry| entry.f)
    }

    fn pop_current(&mut self) -> Option<BlockPos> {
        while let Some(entry) = self.open.pop() {
            if self
                .g_score
                .get(&entry.pos)
                .is_some_and(|current_g| *current_g == entry.g)
            {
                return Some(entry.pos);
            }
        }

        None
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct QueueEntry {
    pos: BlockPos,
    g: u32,
    h: u32,
    f: u32,
}

impl Ord for QueueEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other.f.cmp(&self.f).then_with(|| other.h.cmp(&self.h))
    }
}

impl PartialOrd for QueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn movement_path_steps(path: &[BlockPos], start: BlockPos) -> VecDeque<BlockPos> {
    let mut steps = VecDeque::new();
    let mut last_kept = start;

    for pos in path.iter().skip(1).copied() {
        let advances_xz = pos.0.x != last_kept.0.x || pos.0.z != last_kept.0.z;
        let climbs_up = pos.0.y > last_kept.0.y;
        if !advances_xz && !climbs_up {
            continue;
        }

        last_kept = pos;
        steps.push_back(pos);
    }

    steps
}

pub fn path_height_difference_exceeded(mob_pos: BlockPos, target_pos: BlockPos) -> bool {
    mob_pos.0.y.abs_diff(target_pos.0.y) > MAX_PATH_HEIGHT_DIFFERENCE as u32
}

pub fn block_distance_squared(a: BlockPos, b: BlockPos) -> i64 {
    let dx = i128::from(a.0.x) - i128::from(b.0.x);
    let dy = i128::from(a.0.y) - i128::from(b.0.y);
    let dz = i128::from(a.0.z) - i128::from(b.0.z);
    let distance_squared = dx * dx + dy * dy + dz * dz;
    distance_squared.min(i128::from(i64::MAX)) as i64
}

pub fn horizontal_block_distance(a: BlockPos, b: BlockPos) -> i64 {
    let dx = i64::from(a.0.x).abs_diff(i64::from(b.0.x));
    let dz = i64::from(a.0.z).abs_diff(i64::from(b.0.z));
    i64::try_from(dx.saturating_add(dz)).unwrap_or(i64::MAX)
}

pub fn block_center_feet_pos(pos: BlockPos) -> Vector3<f64> {
    Vector3::new(
        f64::from(pos.0.x) + 0.5,
        f64::from(pos.0.y),
        f64::from(pos.0.z) + 0.5,
    )
}

pub fn path_interval_ticks(horizontal_distance: i64) -> i64 {
    PATH_INTERVAL_HORIZONTAL_DISTANCE_MULTIPLIER_TICKS
        .saturating_mul(horizontal_distance)
        .max(MIN_PATH_INTERVAL_TICKS)
}

#[cfg(test)]
fn next_horizontal_path_step(path: &[BlockPos], start: BlockPos) -> Option<BlockPos> {
    path.iter()
        .skip(1)
        .copied()
        .find(|pos| pos.0.x != start.0.x || pos.0.z != start.0.z)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn pos(x: i32, y: i32, z: i32) -> BlockPos {
        BlockPos::new(x, y, z)
    }

    fn open_grid(bounds: PathBounds) -> BlockGrid {
        BlockGrid::sample(bounds, |_| false).unwrap()
    }

    fn grid_with_closed(bounds: PathBounds, closed: &[BlockPos]) -> BlockGrid {
        let closed = closed.iter().copied().collect::<HashSet<_>>();
        BlockGrid::sample(bounds, |pos| closed.contains(&pos)).unwrap()
    }

    #[test]
    fn cuboid_expands_by_outset() {
        let bounds = PathBounds::between(pos(5, 3, -2), pos(8, 9, 4), 3);

        assert_eq!(bounds.min, pos(2, 0, -5));
        assert_eq!(bounds.max, pos(11, 12, 7));
    }

    #[test]
    fn outside_box_is_rejected() {
        let grid = open_grid(PathBounds::between(pos(0, 0, 0), pos(2, 0, 0), 0));

        assert!(grid.is_open(pos(1, 0, 0)));
        assert!(!grid.is_open(pos(1, 1, 0)));
        assert!(!grid.is_open(pos(-1, 0, 0)));
    }

    #[test]
    fn grid_sampling_tracks_closed_blocks() {
        let grid = grid_with_closed(
            PathBounds::between(pos(0, 0, 0), pos(2, 0, 0), 0),
            &[pos(1, 0, 0)],
        );

        assert!(grid.is_open(pos(0, 0, 0)));
        assert!(!grid.is_open(pos(1, 0, 0)));
        assert!(grid.is_open(pos(2, 0, 0)));
    }

    #[test]
    fn grid_sampling_indexes_y_and_z_correctly() {
        let grid = grid_with_closed(
            PathBounds::between(pos(0, 0, 0), pos(1, 1, 1), 0),
            &[pos(0, 1, 0), pos(1, 0, 1)],
        );

        assert!(!grid.is_open(pos(0, 1, 0)));
        assert!(!grid.is_open(pos(1, 0, 1)));
        assert!(grid.is_open(pos(1, 1, 0)));
        assert!(grid.is_open(pos(0, 0, 1)));
    }

    #[test]
    fn grid_sampling_rejects_oversized_boxes() {
        let bounds = PathBounds::between(pos(0, 0, 0), pos(MAX_PATH_GRID_VOLUME as i32, 0, 0), 0);

        assert!(BlockGrid::sample(bounds, |_| false).is_none());
    }

    #[test]
    fn height_difference_over_eight_skips_pathing() {
        assert!(!path_height_difference_exceeded(pos(0, 0, 0), pos(0, 8, 0)));
        assert!(path_height_difference_exceeded(pos(0, 0, 0), pos(0, 9, 0)));
        assert!(path_height_difference_exceeded(pos(0, 9, 0), pos(0, 0, 0)));
    }

    #[test]
    fn path_interval_uses_horizontal_distance_with_minimum() {
        assert_eq!(path_interval_ticks(0), 5);
        assert_eq!(path_interval_ticks(1), 5);
        assert_eq!(path_interval_ticks(2), 5);
        assert_eq!(path_interval_ticks(3), 6);
        assert_eq!(path_interval_ticks(10), 20);
    }

    #[test]
    fn horizontal_distance_ignores_y() {
        assert_eq!(horizontal_block_distance(pos(0, 0, 0), pos(3, 99, -4)), 7);
    }

    #[test]
    fn next_horizontal_step_skips_y_only_steps() {
        let path = vec![pos(0, 0, 0), pos(0, 1, 0), pos(1, 1, 0)];

        assert_eq!(
            next_horizontal_path_step(&path, pos(0, 0, 0)),
            Some(pos(1, 1, 0))
        );
    }

    #[test]
    fn next_horizontal_step_returns_none_for_y_only_path() {
        let path = vec![pos(0, 0, 0), pos(0, 1, 0), pos(0, 2, 0)];

        assert_eq!(next_horizontal_path_step(&path, pos(0, 0, 0)), None);
    }

    #[test]
    fn movement_path_steps_keep_xz_progress_and_upward_steps() {
        let path = vec![
            pos(0, 0, 0),
            pos(0, 1, 0),
            pos(1, 1, 0),
            pos(1, 2, 0),
            pos(1, 2, 1),
        ];

        assert_eq!(
            movement_path_steps(&path, pos(0, 0, 0)),
            VecDeque::from([pos(0, 1, 0), pos(1, 1, 0), pos(1, 2, 0), pos(1, 2, 1)])
        );
    }

    #[test]
    fn movement_path_steps_still_skip_downward_y_only_steps() {
        let path = vec![pos(0, 2, 0), pos(0, 1, 0), pos(1, 1, 0)];

        assert_eq!(
            movement_path_steps(&path, pos(0, 2, 0)),
            VecDeque::from([pos(1, 1, 0)])
        );
    }

    #[test]
    fn straight_open_path_succeeds() {
        let grid = open_grid(PathBounds::between(pos(0, 0, 0), pos(3, 0, 0), 0));
        let path = bidirectional_a_star(&grid, pos(0, 0, 0), pos(3, 0, 0)).unwrap();

        assert_eq!(path.first().copied(), Some(pos(0, 0, 0)));
        assert_eq!(path.last().copied(), Some(pos(3, 0, 0)));
        assert_eq!(path.get(1).copied(), Some(pos(1, 0, 0)));
    }

    #[test]
    fn open_square_uses_diagonal_xz_route() {
        let grid = open_grid(PathBounds::between(pos(0, 0, 0), pos(3, 0, 3), 0));
        let path = bidirectional_a_star(&grid, pos(0, 0, 0), pos(3, 0, 3)).unwrap();

        assert_eq!(
            path,
            vec![pos(0, 0, 0), pos(1, 0, 1), pos(2, 0, 2), pos(3, 0, 3)]
        );
    }

    #[test]
    fn diagonal_xz_route_does_not_cut_blocked_corners() {
        let grid = grid_with_closed(
            PathBounds::between(pos(0, 0, 0), pos(1, 0, 1), 0),
            &[pos(1, 0, 0)],
        );

        assert!(
            !grid
                .neighbors(pos(0, 0, 0), true)
                .iter()
                .any(|(neighbor, _)| *neighbor == pos(1, 0, 1))
        );
    }

    #[test]
    fn solid_wall_blocks_path() {
        let grid = grid_with_closed(
            PathBounds::between(pos(0, 0, 0), pos(2, 0, 0), 0),
            &[pos(1, 0, 0)],
        );

        assert!(bidirectional_a_star(&grid, pos(0, 0, 0), pos(2, 0, 0)).is_none());
    }

    #[test]
    fn path_cannot_leave_box() {
        let grid = grid_with_closed(
            PathBounds::between(pos(0, 0, 0), pos(2, 0, 0), 0),
            &[pos(1, 0, 0)],
        );

        assert!(grid.is_open(pos(0, 0, 0)));
        assert!(!grid.is_open(pos(0, 1, 0)));
        assert!(bidirectional_a_star(&grid, pos(0, 0, 0), pos(2, 0, 0)).is_none());
    }

    #[test]
    fn upward_movement_cannot_skip_a_block() {
        let grid = grid_with_closed(
            PathBounds::between(pos(0, 0, 0), pos(0, 2, 0), 0),
            &[pos(0, 1, 0)],
        );

        assert!(bidirectional_a_star(&grid, pos(0, 0, 0), pos(0, 2, 0)).is_none());
    }

    #[test]
    fn backward_downward_constraint_matches_forward_climb_rule() {
        let grid = grid_with_closed(
            PathBounds::between(pos(0, 0, 0), pos(0, 2, 0), 0),
            &[pos(0, 1, 0)],
        );

        assert_eq!(
            grid.neighbors(pos(0, 2, 0), false),
            Vec::<(BlockPos, u32)>::new()
        );
    }

    #[test]
    fn returned_next_step_is_open_and_adjacent_to_start() {
        let grid = open_grid(PathBounds::between(pos(0, 0, 0), pos(0, 0, 2), 0));
        let path = bidirectional_a_star(&grid, pos(0, 0, 0), pos(0, 0, 2)).unwrap();
        let next = path[1];

        assert!(grid.is_open(next));
        assert_eq!(block_distance_squared(pos(0, 0, 0), next), 1);
    }
}
