use pumpkin_util::math::vector3::Vector3;
use uuid::Uuid;
use crate::mob_ai::types::{MobLocationTable, MobLocationEntry, ClusterCell};

pub const CLUSTER_DIAMETER_BLOCKS: f64 = 2.0;
pub const CLUSTER_CENTER_PUSH_RADIUS_BLOCKS: f64 = 1.0;
pub const CLUSTER_CELL_SIZE_BLOCKS: f64 = CLUSTER_DIAMETER_BLOCKS;

#[derive(Clone, Debug)]
pub struct MobCluster {
    pub center: Vector3<f64>,
    pub members: Vec<MobClusterMember>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MobClusterMember {
    pub uuid: Uuid,
    pub offset_from_center: Vector3<f64>,
}

impl MobLocationTable {
    pub fn from_entries(entries: impl IntoIterator<Item = MobLocationEntry>) -> Self {
        let mut table = Self::default();

        for entry in entries {
            let cell = cluster_cell(entry.world_uuid, entry.pos);
            table.cells.entry(cell).or_default().push(entry.uuid);
            table.entries.insert(entry.uuid, entry);
        }

        table
    }

    pub fn query_cluster(&self, uuid: Uuid) -> Option<MobCluster> {
        let entry = self.entries.get(&uuid).copied()?;
        let center_cell = cluster_cell(entry.world_uuid, entry.pos);
        let mut candidates = Vec::new();
        let max_distance_squared = CLUSTER_DIAMETER_BLOCKS * CLUSTER_DIAMETER_BLOCKS;

        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let cell = ClusterCell {
                        world_uuid: entry.world_uuid,
                        x: center_cell.x + dx,
                        y: center_cell.y + dy,
                        z: center_cell.z + dz,
                    };

                    let Some(uuids) = self.cells.get(&cell) else {
                        continue;
                    };

                    for candidate_uuid in uuids {
                        let Some(candidate) = self.entries.get(candidate_uuid).copied() else {
                            continue;
                        };

                        if vector_distance_squared(entry.pos, candidate.pos) <= max_distance_squared
                        {
                            candidates.push(candidate);
                        }
                    }
                }
            }
        }

        candidates.sort_by(|a, b| {
            vector_distance_squared(entry.pos, a.pos)
                .total_cmp(&vector_distance_squared(entry.pos, b.pos))
                .then_with(|| a.uuid.cmp(&b.uuid))
        });

        let candidates = diameter_limited_cluster_candidates(entry, candidates);
        if candidates.len() < 2 {
            return None;
        }

        let center = cluster_center(&candidates);
        let center_radius_squared =
            CLUSTER_CENTER_PUSH_RADIUS_BLOCKS * CLUSTER_CENTER_PUSH_RADIUS_BLOCKS;
        let mut members = candidates
            .into_iter()
            .filter_map(|candidate| {
                let offset_from_center = candidate.pos - center;
                (offset_from_center.length_squared() <= center_radius_squared).then_some(
                    MobClusterMember {
                        uuid: candidate.uuid,
                        offset_from_center,
                    },
                )
            })
            .collect::<Vec<_>>();

        if members.len() < 2 {
            return None;
        }

        members.sort_by_key(|member| member.uuid);
        Some(MobCluster { center, members })
    }
}

pub fn cluster_push_velocity(
    uuid: Uuid,
    movement_speed: f64,
    location_table: &MobLocationTable,
) -> Vector3<f64> {
    let Some(cluster) = location_table.query_cluster(uuid) else {
        return Vector3::new(0.0, 0.0, 0.0);
    };
    let _cluster_center = cluster.center;
    let Some(member) = cluster.members.iter().find(|member| member.uuid == uuid) else {
        return Vector3::new(0.0, 0.0, 0.0);
    };

    let mut direction = member.offset_from_center.normalize();
    if direction.length_squared() == 0.0 {
        direction = stable_horizontal_direction(uuid);
    }

    let mut velocity = direction.multiply(movement_speed, movement_speed, movement_speed);
    if velocity.y < 0.0 {
        velocity.y = 0.0;
    }
    velocity
}

pub fn stable_horizontal_direction(uuid: Uuid) -> Vector3<f64> {
    let radians = ((uuid.as_u128() % 360) as f64).to_radians();
    Vector3::new(radians.cos(), 0.0, radians.sin())
}

pub fn cluster_cell(world_uuid: Uuid, pos: Vector3<f64>) -> ClusterCell {
    ClusterCell {
        world_uuid,
        x: cluster_axis_cell(pos.x),
        y: cluster_axis_cell(pos.y),
        z: cluster_axis_cell(pos.z),
    }
}

pub fn cluster_axis_cell(value: f64) -> i32 {
    (value / CLUSTER_CELL_SIZE_BLOCKS).floor() as i32
}

pub fn cluster_center(entries: &[MobLocationEntry]) -> Vector3<f64> {
    let mut sum = Vector3::new(0.0, 0.0, 0.0);

    for entry in entries {
        sum.x += entry.pos.x;
        sum.y += entry.pos.y;
        sum.z += entry.pos.z;
    }

    let count = entries.len() as f64;
    Vector3::new(sum.x / count, sum.y / count, sum.z / count)
}

pub fn vector_distance_squared(a: Vector3<f64>, b: Vector3<f64>) -> f64 {
    (a - b).length_squared()
}

pub fn diameter_limited_cluster_candidates(
    entry: MobLocationEntry,
    candidates: Vec<MobLocationEntry>,
) -> Vec<MobLocationEntry> {
    let max_distance_squared = CLUSTER_DIAMETER_BLOCKS * CLUSTER_DIAMETER_BLOCKS;
    let mut members = vec![entry];

    for candidate in candidates {
        if candidate.uuid == entry.uuid {
            continue;
        }

        if members.iter().all(|member| {
            vector_distance_squared(member.pos, candidate.pos) <= max_distance_squared
        }) {
            members.push(candidate);
        }
    }

    members
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_uuid(value: u128) -> Uuid {
        Uuid::from_u128(value)
    }

    fn location_entry(uuid: Uuid, world_uuid: Uuid, x: f64, y: f64, z: f64) -> MobLocationEntry {
        MobLocationEntry {
            uuid,
            world_uuid,
            pos: Vector3::new(x, y, z),
        }
    }

    #[test]
    fn cluster_table_finds_full_3d_cluster_with_relative_offsets() {
        let world_uuid = test_uuid(100);
        let first_uuid = test_uuid(1);
        let second_uuid = test_uuid(2);
        let table = MobLocationTable::from_entries([
            location_entry(first_uuid, world_uuid, 0.0, 0.0, 0.0),
            location_entry(second_uuid, world_uuid, 2.0, 0.0, 0.0),
        ]);

        let cluster = table.query_cluster(first_uuid).unwrap();

        assert_eq!(cluster.center, Vector3::new(1.0, 0.0, 0.0));
        assert_eq!(
            cluster.members,
            vec![
                MobClusterMember {
                    uuid: first_uuid,
                    offset_from_center: Vector3::new(-1.0, 0.0, 0.0),
                },
                MobClusterMember {
                    uuid: second_uuid,
                    offset_from_center: Vector3::new(1.0, 0.0, 0.0),
                },
            ]
        );
    }

    #[test]
    fn cluster_table_excludes_mobs_more_than_two_blocks_apart_vertically() {
        let world_uuid = test_uuid(100);
        let first_uuid = test_uuid(1);
        let second_uuid = test_uuid(2);
        let table = MobLocationTable::from_entries([
            location_entry(first_uuid, world_uuid, 0.0, 0.0, 0.0),
            location_entry(second_uuid, world_uuid, 0.0, 2.1, 0.0),
        ]);

        assert!(table.query_cluster(first_uuid).is_none());
    }

    #[test]
    fn cluster_table_excludes_mobs_more_than_two_blocks_apart_horizontally() {
        let world_uuid = test_uuid(100);
        let first_uuid = test_uuid(1);
        let second_uuid = test_uuid(2);
        let table = MobLocationTable::from_entries([
            location_entry(first_uuid, world_uuid, 0.0, 0.0, 0.0),
            location_entry(second_uuid, world_uuid, 2.1, 0.0, 0.0),
        ]);

        assert!(table.query_cluster(first_uuid).is_none());
    }

    #[test]
    fn cluster_table_does_not_bridge_chain_into_wide_cluster() {
        let world_uuid = test_uuid(100);
        let left_uuid = test_uuid(1);
        let middle_uuid = test_uuid(2);
        let right_uuid = test_uuid(3);
        let table = MobLocationTable::from_entries([
            location_entry(left_uuid, world_uuid, -1.5, 0.0, 0.0),
            location_entry(middle_uuid, world_uuid, 0.0, 0.0, 0.0),
            location_entry(right_uuid, world_uuid, 1.5, 0.0, 0.0),
        ]);

        let cluster = table.query_cluster(middle_uuid).unwrap();

        assert!(cluster.members.len() < 3);
        assert!(
            cluster
                .members
                .iter()
                .any(|member| member.uuid == middle_uuid)
        );
        for first in &cluster.members {
            for second in &cluster.members {
                assert!(
                    (first.offset_from_center - second.offset_from_center).length_squared()
                        <= CLUSTER_DIAMETER_BLOCKS * CLUSTER_DIAMETER_BLOCKS
                );
            }
        }
    }

    #[test]
    fn cluster_push_adds_outward_movement_speed_velocity() {
        let world_uuid = test_uuid(100);
        let first_uuid = test_uuid(1);
        let second_uuid = test_uuid(2);
        let table = MobLocationTable::from_entries([
            location_entry(first_uuid, world_uuid, -1.0, 0.0, 0.0),
            location_entry(second_uuid, world_uuid, 1.0, 0.0, 0.0),
        ]);

        let velocity = cluster_push_velocity(first_uuid, 0.35, &table);

        assert_eq!(velocity, Vector3::new(-0.35, 0.0, 0.0));
    }

    #[test]
    fn cluster_push_clamps_negative_y_velocity() {
        let world_uuid = test_uuid(100);
        let first_uuid = test_uuid(1);
        let second_uuid = test_uuid(2);
        let table = MobLocationTable::from_entries([
            location_entry(first_uuid, world_uuid, -0.6, -0.6, 0.0),
            location_entry(second_uuid, world_uuid, 0.6, 0.6, 0.0),
        ]);

        let velocity = cluster_push_velocity(first_uuid, 0.5, &table);

        assert!(velocity.x < 0.0);
        assert_eq!(velocity.y, 0.0);
        assert_eq!(velocity.z, 0.0);
    }

    #[test]
    fn cluster_push_uses_stable_nonzero_horizontal_fallback_for_zero_offset() {
        let world_uuid = test_uuid(100);
        let first_uuid = test_uuid(1);
        let second_uuid = test_uuid(2);
        let table = MobLocationTable::from_entries([
            location_entry(first_uuid, world_uuid, 1.0, 1.0, 1.0),
            location_entry(second_uuid, world_uuid, 1.0, 1.0, 1.0),
        ]);

        let velocity = cluster_push_velocity(first_uuid, 0.5, &table);

        assert_eq!(velocity.y, 0.0);
        assert!(velocity.horizontal_length_squared() > 0.0);
        assert!((velocity.horizontal_length() - 0.5).abs() < f64::EPSILON);
    }
}
