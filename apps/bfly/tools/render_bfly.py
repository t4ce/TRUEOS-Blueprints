#!/usr/bin/env python3
"""Render bfly's wing shaders on a Linux EGL render node (uses Pillow)."""

import argparse
import ctypes as C
from functools import cache
from math import sqrt
from pathlib import Path
import re
import shutil
import struct
import subprocess
from tempfile import TemporaryDirectory

from PIL import Image


ROOT = Path(__file__).resolve().parents[1]
LEVELS = (8, 16, 32, 64, 128, 256, 512, 1024)
CACHE_SIZE = LEVELS[-1] + 4
BUTTERFLIES = len(LEVELS)
PANEL_STRIPS = BUTTERFLIES
DISPLAY_STRIPS = PANEL_STRIPS + 2 * BUTTERFLIES
DEFAULT_SIZE = (3072, 1728)
FRAME_RATE = 30
FLIGHT_PERIOD_SECONDS = 12
IDENTITY = (1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)

U, I, P, F = C.c_uint, C.c_int, C.c_void_p, C.c_float
EGL = C.CDLL("libEGL.so.1")
EGL.eglGetProcAddress.argtypes = [C.c_char_p]
EGL.eglGetProcAddress.restype = P

ARRAY_BUFFER, UNIFORM_BUFFER = 0x8892, 0x8A11
SHADER_STORAGE_BUFFER = 0x90D2
STATIC_DRAW, DYNAMIC_DRAW = 0x88E4, 0x88E8
TEXTURE_ARRAY = 0x8C1A
FRAMEBUFFER, COLOR_ATTACHMENT0 = 0x8D40, 0x8CE0
COLOR_BUFFER_BIT = 0x4000
PATCHES, TRIANGLE_STRIP = 0x000E, 0x0005
PRIMITIVES_GENERATED = 0x8C87


@cache
def proc(name, result, *args):
    address = EGL.eglGetProcAddress(name.encode())
    if not address:
        raise RuntimeError(f"Missing OpenGL/EGL entry point {name}")
    return C.CFUNCTYPE(result, *args)(address)


def gl(name, *signature):
    return proc("gl" + name, None, *signature)


def egl(name, result, *args):
    function = getattr(EGL, "egl" + name)
    function.restype, function.argtypes = result, args
    return function


def context(size, render_node):
    devices, count = (P * 16)(), I()
    query = proc("eglQueryDevicesEXT", U, I, C.POINTER(P), C.POINTER(I))
    query_string = proc("eglQueryDeviceStringEXT", C.c_char_p, P, I)
    if not query(len(devices), devices, C.byref(count)):
        raise RuntimeError("EGL could not enumerate render devices")
    device = next((device for device in devices[:count.value]
                   if query_string(device, 0x3377) == render_node.encode()), None)
    if device is None:
        raise RuntimeError(f"EGL cannot open render node {render_node}")
    display = proc("eglGetPlatformDisplayEXT", P, U, P, P)(0x313F, device, None)
    if not egl("Initialize", U, P, P, P)(display, None, None):
        raise RuntimeError(f"EGL could not initialize {render_node}")
    if not egl("BindAPI", U, U)(0x30A2):  # EGL_OPENGL_API
        raise RuntimeError("EGL does not support desktop OpenGL")
    # Pbuffer, desktop OpenGL, RGBA8.
    config_attributes = (I * 13)(0x3033, 1, 0x3040, 8, 0x3024, 8,
                                 0x3023, 8, 0x3022, 8, 0x3021, 8, 0x3038)
    config = P()
    choose = egl("ChooseConfig", U, P, P, P, I, P)
    if not choose(display, config_attributes, C.byref(config), 1, C.byref(count)) or count.value != 1:
        raise RuntimeError("EGL does not expose an OpenGL RGBA8 pbuffer")
    surface_attributes = (I * 5)(0x3057, size[0], 0x3056, size[1], 0x3038)
    surface = egl("CreatePbufferSurface", P, P, P, P)(display, config, surface_attributes)
    context_attributes = (I * 7)(0x3098, 4, 0x30FB, 5, 0x30FD, 1, 0x3038)
    handle = egl("CreateContext", P, P, P, P, P)(display, config, None, context_attributes)
    make_current = egl("MakeCurrent", U, P, P, P, P)
    if not surface or not handle or not make_current(display, surface, surface, handle):
        raise RuntimeError("EGL could not create an OpenGL 4.5 context")
    return display, surface, handle


def check_gl(stage):
    error = proc("glGetError", U)()
    if error:
        raise RuntimeError(f"{stage} failed with OpenGL error {error:#x}")


def create_buffer(create):
    name = U()
    gl(create, I, P)(1, C.byref(name))
    return name


def compile_program(name, stages, varyings=()):
    program = proc("glCreateProgram", U)()
    shaders = []
    for stage, suffix in stages:
        shader = proc("glCreateShader", U, U)(stage)
        code = C.c_char_p((ROOT / "shaders" / f"{name}.{suffix}").read_bytes())
        gl("ShaderSource", U, I, P, P)(shader, 1, C.byref(code), None)
        gl("CompileShader", U)(shader)
        check_status(shader, 0x8B81, "Shader", f"{name}.{suffix}")
        gl("AttachShader", U, U)(program, shader)
        shaders.append(shader)
    if varyings:
        names = (C.c_char_p * len(varyings))(*varyings)
        gl("TransformFeedbackVaryings", U, I, P, U)(program, len(names), names, 0x8C8C)
    gl("LinkProgram", U)(program)
    check_status(program, 0x8B82, "Program", name)
    for shader in shaders:
        gl("DeleteShader", U)(shader)
    return program


def check_status(name, flag, kind, label):
    value = I()
    gl("Get" + kind + "iv", U, U, P)(name, flag, C.byref(value))
    if not value.value:
        log = C.create_string_buffer(4096)
        gl("Get" + kind + "InfoLog", U, I, P, P)(name, len(log), None, log)
        raise RuntimeError(f"{label}: {log.value.decode()}")


def cubic_bounds(values):
    """Include exact interior extrema, not the extrema of the control hull."""
    a, b, c, d = values
    quadratic, linear, constant = d + 3 * b - 3 * c - a, 2 * (c - 2 * b + a), b - a
    roots = []
    if abs(quadratic) < 1e-12:
        if abs(linear) >= 1e-12:
            roots = [-constant / linear]
    else:
        discriminant = linear * linear - 4 * quadratic * constant
        if discriminant >= 0:
            roots = [(-linear + sign * sqrt(discriminant)) / (2 * quadratic) for sign in (-1, 1)]
    extrema = [a, d]
    for t in roots:
        if 0 < t < 1:
            u = 1 - t
            extrema.append(a * u**3 + 3 * b * u * u * t + 3 * c * u * t * t + d * t**3)
    return min(extrema), max(extrema)


def wing_control_points():
    source = (ROOT / "src/geometry.rs").read_text()
    controls, cubics = [], []
    for name, count in (("TOP_WING", 7), ("BOTTOM_WING", 4)):
        path = re.search(rf"const {name}: .*? = \[(.*?)\n\];", source, re.S)
        if not path:
            raise ValueError(f"Could not read control points in {name}")
        points = [(float(x), float(y)) for x, y in re.findall(
            r"\[([-+0-9.eE]+),\s*([-+0-9.eE]+)\]", path.group(1))]
        if len(points) != count:
            raise ValueError(f"Unexpected point count in {name}")
        root_x = points[0][0]
        points = [(x - root_x, 0.45 - y) for x, y in points]
        for start in range(0, count - 1, 3):
            cubic = points[start:start + 4]
            cubics.append(cubic)
            controls.extend((x, y, 0) for x, y in cubic)
        first, last = points[0], points[-1]
        controls.extend((x, y, 1) for x, y in (last, last, first, first))
    xmax = max(cubic_bounds([point[0] for point in cubic])[1] for cubic in cubics)
    y_bounds = [cubic_bounds([point[1] for point in cubic]) for cubic in cubics]
    bounds = (0, min(y[0] for y in y_bounds), xmax, max(y[1] for y in y_bounds))
    return controls, bounds


def upload_controls(controls, *, packed=False):
    vao = create_buffer("GenVertexArrays")
    vbo = create_buffer("GenBuffers")
    gl("BindVertexArray", U)(vao)
    gl("BindBuffer", U, U)(ARRAY_BUFFER, vbo)
    data = struct.pack(f"={len(controls) * 3}f", *(value for row in controls for value in row))
    gl("BufferData", U, C.c_ssize_t, P, U)(ARRAY_BUFFER, len(data), data, STATIC_DRAW)
    attributes = ((0, 3, 0),) if packed else ((0, 2, 0), (1, 1, 8))
    for slot, components, offset in attributes:
        gl("VertexAttribPointer", U, I, U, C.c_ubyte, I, P)(
            slot, components, 0x1406, False, 12, offset)
        gl("EnableVertexAttribArray", U)(slot)
    gl("PatchParameteri", U, I)(0x8E72, 4)  # GL_PATCH_VERTICES
    return vao, vbo


def update_view(*values):
    data = struct.pack("=40f", *values)
    gl("BufferSubData", U, C.c_ssize_t, C.c_ssize_t, P)(UNIFORM_BUFFER, 0, len(data), data)


def draw(topology, vertices, instances, expected=None, query=None):
    if expected is not None:
        gl("BeginQuery", U, U)(PRIMITIVES_GENERATED, query)
    gl("DrawArraysInstanced", U, I, I, I)(topology, 0, vertices, instances)
    if expected is not None:
        gl("EndQuery", U)(PRIMITIVES_GENERATED)
        count = U()
        gl("GetQueryObjectuiv", U, U, P)(query, 0x8866, C.byref(count))
        if count.value != expected:
            raise RuntimeError(f"GPU produced {count.value} primitives, expected {expected}")


def bake_masks(bounds, vertex_count, query):
    texture = create_buffer("GenTextures")
    gl("BindTexture", U, U)(TEXTURE_ARRAY, texture)
    gl("TexStorage3D", U, I, U, I, I, I)(
        TEXTURE_ARRAY, CACHE_SIZE.bit_length(), 0x8229, CACHE_SIZE, CACHE_SIZE, len(LEVELS))
    for parameter, value in ((0x2801, 0x2703), (0x2800, 0x2601), (0x2802, 0x812F), (0x2803, 0x812F)):
        gl("TexParameteri", U, U, I)(TEXTURE_ARRAY, parameter, value)
    fbo = create_buffer("GenFramebuffers")
    gl("BindFramebuffer", U, U)(FRAMEBUFFER, fbo)
    gl("ClearColor", F, F, F, F)(0, 0, 0, 0)
    xmin, ymin, xmax, ymax = bounds
    for layer, segments in enumerate(LEVELS):
        gl("FramebufferTextureLayer", U, U, U, I, I)(FRAMEBUFFER, COLOR_ATTACHMENT0, texture, 0, layer)
        if proc("glCheckFramebufferStatus", U, U)(FRAMEBUFFER) != 0x8CD5:
            raise RuntimeError(f"R8 cache layer {layer} is not renderable")
        viewport = segments + 4
        extent = (segments + 1) / viewport
        sx, sy = 2 * extent / (xmax - xmin), 2 * extent / (ymax - ymin)
        # Center the active shape inside a 1.5-texel border in the layer.
        projection = (sx, 0, 0, 0, 0, sy, 0, 0, 0, 0, 0, 0,
                      -extent - sx * xmin, -extent - sy * ymin, 0, 1)
        update_view(*projection, *bounds, viewport, viewport, 0, 0, *IDENTITY)
        gl("Viewport", I, I, I, I)(0, 0, viewport, viewport)
        gl("Clear", U)(COLOR_BUFFER_BIT)
        draw(PATCHES, vertex_count, 1, 3 * segments + 2, query)
    gl("BindFramebuffer", U, U)(FRAMEBUFFER, 0)
    gl("GenerateMipmap", U)(TEXTURE_ARRAY)
    check_gl("bake R8 cache")
    return texture


def scene_projection(size, focal):
    near, far = 0.25, 2048
    sx, sy = (2 * focal / dimension for dimension in size)
    return (sx, 0, 0, 0, 0, sy, 0, 0, 0, 0, -(far + near) / (far - near), -1,
            0, 0, -2 * near * far / (far - near), 0)


def read_frame(size):
    rgba = (C.c_ubyte * (4 * size[0] * size[1]))()
    gl("ReadPixels", I, I, I, I, U, U, P)(0, 0, *size, 0x1908, 0x1401, rgba)
    check_gl("read display framebuffer")
    return Image.frombytes("RGBA", size, bytes(rgba)).transpose(Image.Transpose.FLIP_TOP_BOTTOM)


def update_time(time):
    # Only framebuffer.z changes between presentation frames. framebuffer.w
    # holds the nearest panel's camera distance.
    now = struct.pack("=f", time)
    gl("BufferSubData", U, C.c_ssize_t, C.c_ssize_t, P)(UNIFORM_BUFFER, 152, len(now), now)


def display_frame(size, time, strips=DISPLAY_STRIPS):
    update_time(time)
    gl("Clear", U)(COLOR_BUFFER_BIT)
    draw(TRIANGLE_STRIP, 4, strips)
    return read_frame(size)


def observed_panel_layers(time, query):
    update_time(time)
    feedback_target = 0x8C8E  # GL_TRANSFORM_FEEDBACK_BUFFER
    discard = 0x8C89  # GL_RASTERIZER_DISCARD
    vertices_per_strip = 6  # Two unindexed triangles in the feedback stream.
    indices = (I * (DISPLAY_STRIPS * vertices_per_strip))()
    feedback = create_buffer("GenBuffers")
    gl("BindBuffer", U, U)(feedback_target, feedback)
    gl("BufferData", U, C.c_ssize_t, P, U)(feedback_target, C.sizeof(indices), None, 0x88E1)
    gl("BindBufferBase", U, U, U)(feedback_target, 0, feedback)
    gl("Enable", U)(discard)
    gl("BeginTransformFeedback", U)(0x0004)  # GL_TRIANGLES
    try:
        draw(TRIANGLE_STRIP, 4, DISPLAY_STRIPS, 2 * DISPLAY_STRIPS, query)
    finally:
        gl("EndTransformFeedback")()
        gl("Disable", U)(discard)
    gl("GetBufferSubData", U, C.c_ssize_t, C.c_ssize_t, P)(
        feedback_target, 0, C.sizeof(indices), indices)
    gl("DeleteBuffers", I, P)(1, C.byref(feedback))
    check_gl("read panel mask layers")

    output = list(indices)
    layers = output[::vertices_per_strip]
    if output != [layer for layer in layers for _ in range(vertices_per_strip)]:
        raise RuntimeError("The vertices of a strip selected inconsistent mask layers")
    if layers[PANEL_STRIPS:] != [layer for layer in layers[:PANEL_STRIPS] for _ in range(2)]:
        raise RuntimeError("A panel and its two wing strips selected inconsistent mask layers")
    return layers[:PANEL_STRIPS]


def save_animation(output, ffmpeg, size, frames, first, later, render_frame):
    # Stream frames to the encoder rather than retaining full RGBA images.
    with TemporaryDirectory(dir=output.parent) as temporary:
        animation = Path(temporary) / "animation.png"
        command = [
            ffmpeg, "-v", "error", "-f", "rawvideo", "-pixel_format", "rgba",
            "-video_size", f"{size[0]}x{size[1]}", "-framerate", str(FRAME_RATE),
            "-i", "pipe:0", "-f", "apng", "-plays", "0", str(animation),
        ]
        with subprocess.Popen(command, stdin=subprocess.PIPE) as encoder:
            try:
                with encoder.stdin as video:
                    video.write(first.tobytes())
                    video.write(later.tobytes())
                    for frame in range(2, frames):
                        video.write(render_frame(size, frame / FRAME_RATE).tobytes())
            except BrokenPipeError as error:
                raise RuntimeError("ffmpeg could not encode the wing animation") from error
            if encoder.wait():
                raise RuntimeError("ffmpeg could not encode the wing animation")
        animation.replace(output)


def render_direct(args, size, controls):
    upload_controls(controls, packed=True)
    camera = create_buffer("GenBuffers")
    gl("BindBuffer", U, U)(SHADER_STORAGE_BUFFER, camera)
    gl("BufferData", U, C.c_ssize_t, P, U)(SHADER_STORAGE_BUFFER, 368, None, DYNAMIC_DRAW)
    gl("BindBufferBase", U, U, U)(SHADER_STORAGE_BUFFER, 0, camera)
    program = compile_program("direct-wings", ((0x8B31, "vert"), (0x8E88, "tesc"),
                                              (0x8E87, "tese"), (0x8B30, "frag")))
    gl("UseProgram", U)(program)
    gl("Viewport", I, I, I, I)(0, 0, *size)
    gl("ClearColor", F, F, F, F)(8 / 255, 10 / 255, 12 / 255, 1)

    def frame(size, time, expected=None, query=None):
        data = struct.pack("=4f", *size, time, FLIGHT_PERIOD_SECONDS)
        gl("BufferSubData", U, C.c_ssize_t, C.c_ssize_t, P)(
            SHADER_STORAGE_BUFFER, 288, len(data), data)
        gl("Clear", U)(COLOR_BUFFER_BIT)
        draw(PATCHES, len(controls), 2 * BUTTERFLIES, expected, query)
        return read_frame(size)

    query = create_buffer("GenQueries")
    frame((1, 1), 0.0, 0, query)
    first = frame(size, 0.0)
    later = frame(size, 1 / FRAME_RATE)
    if size in ((1920, 1080), (1080, 1920), DEFAULT_SIZE, DEFAULT_SIZE[::-1]):
        if first.tobytes() == later.tobytes():
            raise RuntimeError("The direct flight shaders did not animate")
        if frame(size, 2.0).tobytes() == first.tobytes():
            raise RuntimeError("The butterfly pose did not advance after a complete wingbeat")
        flight_closed = frame(size, FLIGHT_PERIOD_SECONDS)
        if flight_closed.tobytes() != first.tobytes():
            raise RuntimeError("The direct flight shaders did not return to their initial pose")
        print(f"Direct flight shaders animated and closed their {FLIGHT_PERIOD_SECONDS}-second lap")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    first.save(args.output.with_suffix(".png"))
    if args.frames > 1:
        animation = args.output.with_name(args.output.stem + "-animated.png")
        save_animation(animation, args.ffmpeg, size, args.frames, first, later, frame)


def render(args):
    size = tuple(args.size)
    display, surface, handle = context(size, args.render_node)
    try:
        renderer = proc("glGetString", C.c_char_p, U)(0x1F01).decode()
        print(f"renderer={renderer}")
        controls, bounds = wing_control_points()
        if args.direct:
            render_direct(args, size, controls)
            return
        upload_controls(controls)
        view = create_buffer("GenBuffers")
        gl("BindBuffer", U, U)(UNIFORM_BUFFER, view)
        gl("BufferData", U, C.c_ssize_t, P, U)(UNIFORM_BUFFER, 160, None, DYNAMIC_DRAW)
        gl("BindBufferBase", U, U, U)(UNIFORM_BUFFER, 0, view)
        query = create_buffer("GenQueries")
        bake = compile_program("wings", ((0x8B31, "vert"), (0x8E88, "tesc"),
                                         (0x8E87, "tese"), (0x8B30, "frag")))
        gl("UseProgram", U)(bake)
        texture = bake_masks(bounds, len(controls), query)
        message = ", ".join(str(n) for n in LEVELS)
        print(f"R8 cache baked once at {message} segments per Bézier")

        empty_vao = create_buffer("GenVertexArrays")
        gl("BindVertexArray", U)(empty_vao)
        gl("ActiveTexture", U)(0x84C1)  # GL_TEXTURE1
        gl("BindTexture", U, U)(TEXTURE_ARRAY, texture)
        program = compile_program("cached-wings", ((0x8B31, "vert"), (0x8B30, "frag")), (b"mask_layer",))
        gl("UseProgram", U)(program)
        gl("Viewport", I, I, I, I)(0, 0, *size)
        gl("ClearColor", F, F, F, F)(0, 0, 0, 1)
        gl("Enable", U)(0x0BE2)  # GL_BLEND
        gl("BlendFunc", U, U)(1, 0x0303)  # GL_ONE, GL_ONE_MINUS_SRC_ALPHA
        camera_distance = 6.0
        update_view(*scene_projection(size, 1.0), *IDENTITY, *bounds, *size, 0, camera_distance)
        if observed_panel_layers(0.0, query) != [-1] * BUTTERFLIES:
            raise RuntimeError("Sub-4-pixel butterflies selected visible mask layers")
        update_view(*scene_projection(size, 8000), *IDENTITY, *bounds, *size, 0, camera_distance)
        layers = [observed_panel_layers(time, query) for time in (0.0, 0.5, 1.0, 1.5)]
        if size in (DEFAULT_SIZE, DEFAULT_SIZE[::-1]):
            if any(sample[0] != len(LEVELS) - 1 or sample[-1] < 0
                   or sample != sorted(sample, reverse=True) for sample in layers):
                raise RuntimeError(f"Demo panels selected unexpected mask layers {layers}")
        elected = [LEVELS[layer] if layer >= 0 else "culled" for layer in layers[0]]
        print(f"Display panel sampling: {', '.join(map(str, elected))}")
        background = display_frame(size, 0.0, PANEL_STRIPS)
        first = display_frame(size, 0.0)
        if first.tobytes() == background.tobytes():
            raise RuntimeError("No cached wing masks reached the display framebuffer")
        later = display_frame(size, 1 / FRAME_RATE)
        if first.tobytes() == later.tobytes():
            raise RuntimeError("The outer corners did not animate")
        print(f"One display draw generated {4 * BUTTERFLIES} wing triangles "
              f"and {2 * PANEL_STRIPS} panel triangles; culling and animation passed")

        args.output.parent.mkdir(parents=True, exist_ok=True)
        first.save(args.output.with_suffix(".png"))
        if args.frames > 1:
            animation = args.output.with_name(args.output.stem + "-animated.png")
            save_animation(animation, args.ffmpeg, size, args.frames, first, later, display_frame)
    finally:
        egl("MakeCurrent", U, P, P, P, P)(display, None, None, None)
        egl("DestroyContext", U, P, P)(display, handle)
        egl("DestroySurface", U, P, P)(display, surface)
        egl("Terminate", U, P)(display)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--direct", action="store_true", help="draw the GPU-tessellated outlines")
    parser.add_argument("--render-node", default="/dev/dri/renderD128")
    parser.add_argument("--size", type=int, nargs=2, metavar=("WIDTH", "HEIGHT"),
                        default=DEFAULT_SIZE, help="backing framebuffer dimensions")
    parser.add_argument("--output", type=Path, help="output PNG path")
    parser.add_argument("--frames", type=int, default=1, help="number of frames sampled at 30 Hz")
    args = parser.parse_args()
    if min(args.size) < 1:
        parser.error("--size dimensions must be positive")
    if args.frames < 1:
        parser.error("--frames must be at least 1")
    if args.output is None:
        filename = "bfly-direct-wings.png" if args.direct else "bfly-panels.png"
        args.output = ROOT / "target" / filename
    if args.frames > 1:
        args.ffmpeg = shutil.which("ffmpeg")
        if args.ffmpeg is None:
            parser.error("ffmpeg is required for animated captures")
    render(args)


if __name__ == "__main__":
    main()
