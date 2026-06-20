use crate::uniforms::Vertex;

/// Procedural debug mesh shapes for quick visualisation without external assets.
#[derive(Clone, Debug, PartialEq)]
pub enum DebugMesh {
    /// Unit cube centred at origin, edge length 1, hard normals.
    Cube,
    /// UV sphere with the given longitudinal/latitudinal segment count.
    Sphere(u32),
    /// Quad on the XY plane from (-0.5, -0.5, 0) to (0.5, 0.5, 0).
    Plane,
}

/// GPU buffers for a single indexed mesh, ready for drawing.
#[derive(Debug)]
pub struct MeshBuffers {
    /// Vertex buffer uploaded to the GPU.
    pub vertex_buffer: wgpu::Buffer,
    /// Index buffer uploaded to the GPU.
    pub index_buffer: wgpu::Buffer,
    /// Number of indices to draw.
    pub index_count: u32,
}

pub(crate) fn mesh_name(mesh: &DebugMesh) -> String {
    match mesh {
        DebugMesh::Cube => "debug/cube".to_string(),
        DebugMesh::Sphere(_) => "debug/sphere".to_string(),
        DebugMesh::Plane => "debug/plane".to_string(),
    }
}

// Cube vertices with hard normals (24 vertices, 4 per face × 6 faces).
pub(crate) const CUBE_VERTICES: &[Vertex] = &[
    // Front face (+Z)
    Vertex {
        position: [-0.5, -0.5, 0.5],
        normal: [0.0, 0.0, 1.0],
        uv: [0.0, 0.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    },
    Vertex {
        position: [0.5, -0.5, 0.5],
        normal: [0.0, 0.0, 1.0],
        uv: [1.0, 0.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    },
    Vertex {
        position: [0.5, 0.5, 0.5],
        normal: [0.0, 0.0, 1.0],
        uv: [1.0, 1.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    },
    Vertex {
        position: [-0.5, 0.5, 0.5],
        normal: [0.0, 0.0, 1.0],
        uv: [0.0, 1.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    },
    // Back face (-Z)
    Vertex {
        position: [0.5, -0.5, -0.5],
        normal: [0.0, 0.0, -1.0],
        uv: [0.0, 0.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    },
    Vertex {
        position: [-0.5, -0.5, -0.5],
        normal: [0.0, 0.0, -1.0],
        uv: [1.0, 0.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    },
    Vertex {
        position: [-0.5, 0.5, -0.5],
        normal: [0.0, 0.0, -1.0],
        uv: [1.0, 1.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    },
    Vertex {
        position: [0.5, 0.5, -0.5],
        normal: [0.0, 0.0, -1.0],
        uv: [0.0, 1.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    },
    // Right face (+X)
    Vertex {
        position: [0.5, -0.5, 0.5],
        normal: [1.0, 0.0, 0.0],
        uv: [0.0, 0.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    },
    Vertex {
        position: [0.5, -0.5, -0.5],
        normal: [1.0, 0.0, 0.0],
        uv: [1.0, 0.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    },
    Vertex {
        position: [0.5, 0.5, -0.5],
        normal: [1.0, 0.0, 0.0],
        uv: [1.0, 1.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    },
    Vertex {
        position: [0.5, 0.5, 0.5],
        normal: [1.0, 0.0, 0.0],
        uv: [0.0, 1.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    },
    // Left face (-X)
    Vertex {
        position: [-0.5, -0.5, -0.5],
        normal: [-1.0, 0.0, 0.0],
        uv: [0.0, 0.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    },
    Vertex {
        position: [-0.5, -0.5, 0.5],
        normal: [-1.0, 0.0, 0.0],
        uv: [1.0, 0.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    },
    Vertex {
        position: [-0.5, 0.5, 0.5],
        normal: [-1.0, 0.0, 0.0],
        uv: [1.0, 1.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    },
    Vertex {
        position: [-0.5, 0.5, -0.5],
        normal: [-1.0, 0.0, 0.0],
        uv: [0.0, 1.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    },
    // Top face (+Y)
    Vertex {
        position: [-0.5, 0.5, 0.5],
        normal: [0.0, 1.0, 0.0],
        uv: [0.0, 0.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    },
    Vertex {
        position: [0.5, 0.5, 0.5],
        normal: [0.0, 1.0, 0.0],
        uv: [1.0, 0.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    },
    Vertex {
        position: [0.5, 0.5, -0.5],
        normal: [0.0, 1.0, 0.0],
        uv: [1.0, 1.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    },
    Vertex {
        position: [-0.5, 0.5, -0.5],
        normal: [0.0, 1.0, 0.0],
        uv: [0.0, 1.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    },
    // Bottom face (-Y)
    Vertex {
        position: [-0.5, -0.5, -0.5],
        normal: [0.0, -1.0, 0.0],
        uv: [0.0, 0.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    },
    Vertex {
        position: [0.5, -0.5, -0.5],
        normal: [0.0, -1.0, 0.0],
        uv: [1.0, 0.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    },
    Vertex {
        position: [0.5, -0.5, 0.5],
        normal: [0.0, -1.0, 0.0],
        uv: [1.0, 1.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    },
    Vertex {
        position: [-0.5, -0.5, 0.5],
        normal: [0.0, -1.0, 0.0],
        uv: [0.0, 1.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    },
];

pub(crate) const CUBE_INDICES: &[u32] = &[
    0, 1, 2, 2, 3, 0, // front
    4, 5, 6, 6, 7, 4, // back
    8, 9, 10, 10, 11, 8, // right
    12, 13, 14, 14, 15, 12, // left
    16, 17, 18, 18, 19, 16, // top
    20, 21, 22, 22, 23, 20, // bottom
];

pub(crate) fn generate_mesh(mesh: &DebugMesh) -> (Vec<Vertex>, Vec<u32>) {
    match mesh {
        DebugMesh::Cube => generate_cube(),
        DebugMesh::Sphere(segments) => generate_sphere(*segments),
        DebugMesh::Plane => generate_plane(),
    }
}

pub(crate) fn generate_cube() -> (Vec<Vertex>, Vec<u32>) {
    let indices = CUBE_INDICES.to_vec();
    (
        with_generated_tangents(CUBE_VERTICES.to_vec(), &indices),
        indices,
    )
}

pub(crate) fn generate_sphere(segments: u32) -> (Vec<Vertex>, Vec<u32>) {
    let segs = segments.max(3);
    let lat = segs;
    let lon = segs * 2;

    let mut vertices = Vec::with_capacity(((lat + 1) * (lon + 1)) as usize);
    let mut indices = Vec::with_capacity((lat * lon * 6) as usize);

    for i in 0..=lat {
        let v = i as f32 / lat as f32;
        let theta = v * std::f32::consts::PI;
        let y = theta.cos();
        let r = theta.sin();

        for j in 0..=lon {
            let u = j as f32 / lon as f32;
            let phi = u * 2.0 * std::f32::consts::PI;
            let x = r * phi.cos();
            let z = r * phi.sin();

            vertices.push(Vertex {
                position: [x * 0.5, y * 0.5, z * 0.5],
                normal: [x, y, z],
                uv: [u, v],
                tangent: [1.0, 0.0, 0.0, 1.0],
            });
        }
    }

    for i in 0..lat {
        for j in 0..lon {
            let a = i * (lon + 1) + j;
            let b = a + lon + 1;
            let c = a + 1;
            let d = b + 1;
            indices.push(a);
            indices.push(b);
            indices.push(c);
            indices.push(c);
            indices.push(b);
            indices.push(d);
        }
    }

    let vertices = with_generated_tangents(vertices, &indices);
    (vertices, indices)
}

pub(crate) fn generate_plane() -> (Vec<Vertex>, Vec<u32>) {
    let vertices = vec![
        Vertex {
            position: [-0.5, -0.5, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [0.0, 0.0],
            tangent: [1.0, 0.0, 0.0, 1.0],
        },
        Vertex {
            position: [0.5, -0.5, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [1.0, 0.0],
            tangent: [1.0, 0.0, 0.0, 1.0],
        },
        Vertex {
            position: [0.5, 0.5, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [1.0, 1.0],
            tangent: [1.0, 0.0, 0.0, 1.0],
        },
        Vertex {
            position: [-0.5, 0.5, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [0.0, 1.0],
            tangent: [1.0, 0.0, 0.0, 1.0],
        },
    ];
    let indices = vec![0, 1, 2, 2, 3, 0];
    let vertices = with_generated_tangents(vertices, &indices);
    (vertices, indices)
}

pub(crate) fn generate_grid() -> Vec<Vertex> {
    let half = 50.0;
    let mut vertices = Vec::with_capacity(404);

    for i in -50..=50 {
        let x = i as f32;
        let alpha = if i % 5 == 0 { 0.35 } else { 0.15 };
        vertices.push(Vertex {
            position: [x, 0.0, -half],
            normal: [0.0, 1.0, 0.0],
            uv: [alpha, 0.0],
            tangent: [1.0, 0.0, 0.0, 1.0],
        });
        vertices.push(Vertex {
            position: [x, 0.0, half],
            normal: [0.0, 1.0, 0.0],
            uv: [alpha, 0.0],
            tangent: [1.0, 0.0, 0.0, 1.0],
        });
    }
    for i in -50..=50 {
        let z = i as f32;
        let alpha = if i % 5 == 0 { 0.35 } else { 0.15 };
        vertices.push(Vertex {
            position: [-half, 0.0, z],
            normal: [0.0, 1.0, 0.0],
            uv: [alpha, 0.0],
            tangent: [1.0, 0.0, 0.0, 1.0],
        });
        vertices.push(Vertex {
            position: [half, 0.0, z],
            normal: [0.0, 1.0, 0.0],
            uv: [alpha, 0.0],
            tangent: [1.0, 0.0, 0.0, 1.0],
        });
    }

    vertices
}

pub(crate) fn with_generated_tangents(mut vertices: Vec<Vertex>, indices: &[u32]) -> Vec<Vertex> {
    let mut accum = vec![[0.0f32; 3]; vertices.len()];

    for tri in indices.chunks_exact(3) {
        let i0 = tri[0] as usize;
        let i1 = tri[1] as usize;
        let i2 = tri[2] as usize;
        if i0 >= vertices.len() || i1 >= vertices.len() || i2 >= vertices.len() {
            continue;
        }

        let p0 = vertices[i0].position;
        let p1 = vertices[i1].position;
        let p2 = vertices[i2].position;
        let uv0 = vertices[i0].uv;
        let uv1 = vertices[i1].uv;
        let uv2 = vertices[i2].uv;

        let edge1 = sub3(p1, p0);
        let edge2 = sub3(p2, p0);
        let duv1 = [uv1[0] - uv0[0], uv1[1] - uv0[1]];
        let duv2 = [uv2[0] - uv0[0], uv2[1] - uv0[1]];
        let det = duv1[0] * duv2[1] - duv2[0] * duv1[1];
        if det.abs() <= 1.0e-6 {
            continue;
        }

        let inv_det = 1.0 / det;
        let tangent = [
            (edge1[0] * duv2[1] - edge2[0] * duv1[1]) * inv_det,
            (edge1[1] * duv2[1] - edge2[1] * duv1[1]) * inv_det,
            (edge1[2] * duv2[1] - edge2[2] * duv1[1]) * inv_det,
        ];

        accum[i0] = add3(accum[i0], tangent);
        accum[i1] = add3(accum[i1], tangent);
        accum[i2] = add3(accum[i2], tangent);
    }

    for (vertex, tangent) in vertices.iter_mut().zip(accum) {
        let normal = vertex.normal;
        let tangent = sub3(tangent, mul3(normal, dot3(normal, tangent)));
        let tangent = normalize3(tangent).unwrap_or_else(|| fallback_tangent(normal));
        vertex.tangent = [tangent[0], tangent[1], tangent[2], 1.0];
    }

    vertices
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn mul3(a: [f32; 3], scalar: f32) -> [f32; 3] {
    [a[0] * scalar, a[1] * scalar, a[2] * scalar]
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize3(v: [f32; 3]) -> Option<[f32; 3]> {
    let len_sq = dot3(v, v);
    if len_sq <= 1.0e-10 {
        return None;
    }
    let inv_len = 1.0 / len_sq.sqrt();
    Some(mul3(v, inv_len))
}

fn fallback_tangent(normal: [f32; 3]) -> [f32; 3] {
    let axis = if normal[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let tangent = sub3(axis, mul3(normal, dot3(normal, axis)));
    normalize3(tangent).unwrap_or([1.0, 0.0, 0.0])
}
