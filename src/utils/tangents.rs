use crate::engine::math::{Vector2, Vector3, Vector4};

/// Computes orthonormal tangent vectors with handedness sign according to MikkTSpace standard.
pub fn generate_tangents(
    positions: &[Vector3],
    normals: &[Vector3],
    uvs: &[Vector2],
    indices: &[u32],
) -> Vec<Vector4> {
    let vertex_count = positions.len();
    if vertex_count == 0 {
        return Vec::new();
    }

    let mut tan1 = vec![Vector3::default(); vertex_count];
    let mut tan2 = vec![Vector3::default(); vertex_count];

    // Compute triangle-level tangent & bitangent vectors
    for tri in indices.as_chunks::<3>().0 {
        let i1 = tri[0] as usize;
        let i2 = tri[1] as usize;
        let i3 = tri[2] as usize;

        if i1 >= vertex_count || i2 >= vertex_count || i3 >= vertex_count {
            continue;
        }

        let v1 = positions[i1];
        let v2 = positions[i2];
        let v3 = positions[i3];

        let w1 = uvs.get(i1).copied().unwrap_or_default();
        let w2 = uvs.get(i2).copied().unwrap_or_default();
        let w3 = uvs.get(i3).copied().unwrap_or_default();

        let x1 = v2.x - v1.x;
        let x2 = v3.x - v1.x;
        let y1 = v2.y - v1.y;
        let y2 = v3.y - v1.y;
        let z1 = v2.z - v1.z;
        let z2 = v3.z - v1.z;

        let s1 = w2.x - w1.x;
        let s2 = w3.x - w1.x;
        let t1 = w2.y - w1.y;
        let t2 = w3.y - w1.y;

        let r = s1 * t2 - s2 * t1;
        let inv_r = if r.abs() > 1e-6 { 1.0 / r } else { 0.0 };

        let sdir = Vector3 {
            x: (t2 * x1 - t1 * x2) * inv_r,
            y: (t2 * y1 - t1 * y2) * inv_r,
            z: (t2 * z1 - t1 * z2) * inv_r,
        };

        let tdir = Vector3 {
            x: (s1 * x2 - s2 * x1) * inv_r,
            y: (s1 * y2 - s2 * y1) * inv_r,
            z: (s1 * z2 - s2 * z1) * inv_r,
        };

        tan1[i1].x += sdir.x;
        tan1[i1].y += sdir.y;
        tan1[i1].z += sdir.z;

        tan1[i2].x += sdir.x;
        tan1[i2].y += sdir.y;
        tan1[i2].z += sdir.z;

        tan1[i3].x += sdir.x;
        tan1[i3].y += sdir.y;
        tan1[i3].z += sdir.z;

        tan2[i1].x += tdir.x;
        tan2[i1].y += tdir.y;
        tan2[i1].z += tdir.z;

        tan2[i2].x += tdir.x;
        tan2[i2].y += tdir.y;
        tan2[i2].z += tdir.z;

        tan2[i3].x += tdir.x;
        tan2[i3].y += tdir.y;
        tan2[i3].z += tdir.z;
    }

    let mut tangents = Vec::with_capacity(vertex_count);

    // Gram-Schmidt orthogonalize & calculate handedness
    for i in 0..vertex_count {
        let n = normals.get(i).copied().unwrap_or(Vector3 {
            x: 0.0,
            y: 1.0,
            z: 0.0,
        });
        let t = tan1[i];

        let dot_nt = n.x * t.x + n.y * t.y + n.z * t.z;
        let mut orth_x = t.x - n.x * dot_nt;
        let mut orth_y = t.y - n.y * dot_nt;
        let mut orth_z = t.z - n.z * dot_nt;

        let len = (orth_x * orth_x + orth_y * orth_y + orth_z * orth_z).sqrt();
        if len > 1e-6 {
            orth_x /= len;
            orth_y /= len;
            orth_z /= len;
        } else {
            orth_x = 1.0;
            orth_y = 0.0;
            orth_z = 0.0;
        }

        // Cross product N x T to determine bitangent sign
        let cross_x = n.y * orth_z - n.z * orth_y;
        let cross_y = n.z * orth_x - n.x * orth_z;
        let cross_z = n.x * orth_y - n.y * orth_x;

        let dot_cross_b = cross_x * tan2[i].x + cross_y * tan2[i].y + cross_z * tan2[i].z;
        let sign = if dot_cross_b < 0.0 { -1.0 } else { 1.0 };

        tangents.push(Vector4 {
            x: orth_x,
            y: orth_y,
            z: orth_z,
            w: sign,
        });
    }

    tangents
}
