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
    },
    Vertex {
        position: [0.5, -0.5, 0.5],
        normal: [0.0, 0.0, 1.0],
        uv: [1.0, 0.0],
    },
    Vertex {
        position: [0.5, 0.5, 0.5],
        normal: [0.0, 0.0, 1.0],
        uv: [1.0, 1.0],
    },
    Vertex {
        position: [-0.5, 0.5, 0.5],
        normal: [0.0, 0.0, 1.0],
        uv: [0.0, 1.0],
    },
    // Back face (-Z)
    Vertex {
        position: [0.5, -0.5, -0.5],
        normal: [0.0, 0.0, -1.0],
        uv: [0.0, 0.0],
    },
    Vertex {
        position: [-0.5, -0.5, -0.5],
        normal: [0.0, 0.0, -1.0],
        uv: [1.0, 0.0],
    },
    Vertex {
        position: [-0.5, 0.5, -0.5],
        normal: [0.0, 0.0, -1.0],
        uv: [1.0, 1.0],
    },
    Vertex {
        position: [0.5, 0.5, -0.5],
        normal: [0.0, 0.0, -1.0],
        uv: [0.0, 1.0],
    },
    // Right face (+X)
    Vertex {
        position: [0.5, -0.5, 0.5],
        normal: [1.0, 0.0, 0.0],
        uv: [0.0, 0.0],
    },
    Vertex {
        position: [0.5, -0.5, -0.5],
        normal: [1.0, 0.0, 0.0],
        uv: [1.0, 0.0],
    },
    Vertex {
        position: [0.5, 0.5, -0.5],
        normal: [1.0, 0.0, 0.0],
        uv: [1.0, 1.0],
    },
    Vertex {
        position: [0.5, 0.5, 0.5],
        normal: [1.0, 0.0, 0.0],
        uv: [0.0, 1.0],
    },
    // Left face (-X)
    Vertex {
        position: [-0.5, -0.5, -0.5],
        normal: [-1.0, 0.0, 0.0],
        uv: [0.0, 0.0],
    },
    Vertex {
        position: [-0.5, -0.5, 0.5],
        normal: [-1.0, 0.0, 0.0],
        uv: [1.0, 0.0],
    },
    Vertex {
        position: [-0.5, 0.5, 0.5],
        normal: [-1.0, 0.0, 0.0],
        uv: [1.0, 1.0],
    },
    Vertex {
        position: [-0.5, 0.5, -0.5],
        normal: [-1.0, 0.0, 0.0],
        uv: [0.0, 1.0],
    },
    // Top face (+Y)
    Vertex {
        position: [-0.5, 0.5, 0.5],
        normal: [0.0, 1.0, 0.0],
        uv: [0.0, 0.0],
    },
    Vertex {
        position: [0.5, 0.5, 0.5],
        normal: [0.0, 1.0, 0.0],
        uv: [1.0, 0.0],
    },
    Vertex {
        position: [0.5, 0.5, -0.5],
        normal: [0.0, 1.0, 0.0],
        uv: [1.0, 1.0],
    },
    Vertex {
        position: [-0.5, 0.5, -0.5],
        normal: [0.0, 1.0, 0.0],
        uv: [0.0, 1.0],
    },
    // Bottom face (-Y)
    Vertex {
        position: [-0.5, -0.5, -0.5],
        normal: [0.0, -1.0, 0.0],
        uv: [0.0, 0.0],
    },
    Vertex {
        position: [0.5, -0.5, -0.5],
        normal: [0.0, -1.0, 0.0],
        uv: [1.0, 0.0],
    },
    Vertex {
        position: [0.5, -0.5, 0.5],
        normal: [0.0, -1.0, 0.0],
        uv: [1.0, 1.0],
    },
    Vertex {
        position: [-0.5, -0.5, 0.5],
        normal: [0.0, -1.0, 0.0],
        uv: [0.0, 1.0],
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
    (CUBE_VERTICES.to_vec(), CUBE_INDICES.to_vec())
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

    (vertices, indices)
}

pub(crate) fn generate_plane() -> (Vec<Vertex>, Vec<u32>) {
    let vertices = vec![
        Vertex {
            position: [-0.5, -0.5, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [0.0, 0.0],
        },
        Vertex {
            position: [0.5, -0.5, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [1.0, 0.0],
        },
        Vertex {
            position: [0.5, 0.5, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [1.0, 1.0],
        },
        Vertex {
            position: [-0.5, 0.5, 0.0],
            normal: [0.0, 0.0, 1.0],
            uv: [0.0, 1.0],
        },
    ];
    let indices = vec![0, 1, 2, 2, 3, 0];
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
        });
        vertices.push(Vertex {
            position: [x, 0.0, half],
            normal: [0.0, 1.0, 0.0],
            uv: [alpha, 0.0],
        });
    }
    for i in -50..=50 {
        let z = i as f32;
        let alpha = if i % 5 == 0 { 0.35 } else { 0.15 };
        vertices.push(Vertex {
            position: [-half, 0.0, z],
            normal: [0.0, 1.0, 0.0],
            uv: [alpha, 0.0],
        });
        vertices.push(Vertex {
            position: [half, 0.0, z],
            normal: [0.0, 1.0, 0.0],
            uv: [alpha, 0.0],
        });
    }

    vertices
}
