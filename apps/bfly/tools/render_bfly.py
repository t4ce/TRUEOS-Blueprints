#!/usr/bin/env python3
"""Render bfly's wing shaders on a Linux EGL render node (uses Pillow)."""

import argparse
import ctypes as C
from functools import cache
from math import isfinite, lcm, sqrt
from pathlib import Path
import re
import shutil
import struct
import subprocess
from tempfile import TemporaryDirectory

from bake_flight_turn import flight_turn_source
from bake_stage_wake import stage_wake_source
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
FLAP_PHASE_PERIOD_SECONDS = 60
FLOCK_PERIOD_SECONDS = 1200
SWAP_PERIOD_SECONDS = 420
MOTION_PERIOD_SECONDS = lcm(FLAP_PHASE_PERIOD_SECONDS, FLOCK_PERIOD_SECONDS, SWAP_PERIOD_SECONDS)
IDENTITY = (1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)

U, I, P, F = C.c_uint, C.c_int, C.c_void_p, C.c_float
EGL = C.CDLL("libEGL.so.1")
EGL.eglGetProcAddress.argtypes = [C.c_char_p]
EGL.eglGetProcAddress.restype = P

ARRAY_BUFFER, UNIFORM_BUFFER = 0x8892, 0x8A11
SHADER_STORAGE_BUFFER = 0x90D2
STATIC_DRAW, DYNAMIC_DRAW = 0x88E4, 0x88E8
TEXTURE_2D, TEXTURE_ARRAY = 0x0DE1, 0x8C1A
FRAMEBUFFER, COLOR_ATTACHMENT0 = 0x8D40, 0x8CE0
COLOR_BUFFER_BIT = 0x4000
PATCHES, TRIANGLE_STRIP = 0x000E, 0x0005
PRIMITIVES_GENERATED = 0x8C87
BLEND, ONE, ONE_MINUS_SRC_ALPHA = 0x0BE2, 1, 0x0303


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


def compile_program(name, stages, varyings=(), *, defines=(), stage_names=None, turn_limited=False):
    program = proc("glCreateProgram", U)()
    shaders = []
    for stage, suffix in stages:
        shader = proc("glCreateShader", U, U)(stage)
        stage_name = stage_names.get(suffix, name) if stage_names else name
        if turn_limited and stage_name == "direct-wings" and suffix == "tesc":
            flight_source = flight_turn_source(ROOT / "shaders")
            if stage_names and stage_names.get("tese") == "sunrise-ribbon":
                flight_source = stage_wake_source(flight_source)
            source = flight_source.encode()
        else:
            source = (ROOT / "shaders" / f"{stage_name}.{suffix}").read_bytes()
        if defines:
            version, newline, rest = source.partition(b"\n")
            prelude = b"".join(b"#define " + define + b"\n" for define in defines)
            source = version + newline + prelude + rest
        code = C.c_char_p(source)
        gl("ShaderSource", U, I, P, P)(shader, 1, C.byref(code), None)
        gl("CompileShader", U)(shader)
        check_status(shader, 0x8B81, "Shader", f"{stage_name}.{suffix}")
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


def wing_control_points(*, direct=False, fill=False, tuned_wake=False):
    source = (ROOT / "src/geometry.rs").read_text()

    def read_points(name, count):
        path = re.search(rf"const {name}: .*? = \[(.*?)\n\];", source, re.S)
        if not path:
            raise ValueError(f"Could not read control points in {name}")
        points = [(float(x), float(y)) for x, y in re.findall(
            r"\[([-+0-9.eE]+),\s*([-+0-9.eE]+)\]", path.group(1))]
        if len(points) != count:
            raise ValueError(f"Unexpected point count in {name}")
        return points

    closed = fill or tuned_wake
    if closed:
        outlines = [((read_points("FILL_WING", 10),), None),
                    ((read_points("ALTERNATE_FILL_WING", 16),), None)]
    else:
        top = read_points("TOP_WING", 7)
        if direct:
            outlines = [((top, read_points("BOTTOM_WING", 4)), read_points("ROOT_CONNECTOR", 2)),
                        ((read_points("ALTERNATE_TOP_WING", 10), read_points("ALTERNATE_BOTTOM_WING", 7)),
                         read_points("ALTERNATE_ROOT_CONNECTOR", 2))]
        else:
            # The cached demo uses the original lower curve and both closures.
            bottom = [
                (0.014813999645411968, 0.44999998807907104),
                (0.9448009133338928, 0.3951452970504761),
                (0.24930904805660248, 1.2085520029067993),
                (0.014813999645411968, 0.4659808874130249),
            ]
            outlines = [((top, bottom), None)]

    controls, cubics = [], []
    for artwork, (paths, connector) in enumerate(outlines):
        tag = (4 if closed else 2) * artwork
        for points in paths:
            root_x = connector[0][0] if connector is not None else points[0][0]
            points = [(x - root_x, 0.45 - y) for x, y in points]
            for start in range(0, len(points) - 1, 3):
                cubic = points[start:start + 4]
                cubics.append(cubic)
                curve_tag = tag + (8 * (start // 3) if tuned_wake else 0)
                controls.extend((x, y, curve_tag) for x, y in cubic)
            if connector is None:
                first, last = points[0], points[-1]
                controls.extend((x, y, tag + 1) for x, y in (last, last, first, first))
        if connector is not None:
            first, last = [(x - connector[0][0], 0.45 - y) for x, y in connector]
            controls.extend((x, y, tag + 1) for x, y in (first, first, last, last))
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


def draw(topology, vertices, instances, expected=None, query=None, *, first=0, tier_widths=False):
    if expected is not None:
        gl("BeginQuery", U, U)(PRIMITIVES_GENERATED, query)
    if tier_widths:
        line_width = gl("LineWidth", F)
        draw_pass = gl("DrawArraysInstancedBaseInstance", U, I, I, I, U)
        for width_class, width in enumerate((2.0, 3.0, 4.0)):
            line_width(width)
            draw_pass(topology, first, vertices, instances, width_class * 256)
    else:
        gl("DrawArraysInstanced", U, I, I, I)(topology, first, vertices, instances)
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


def fill_family(color):
    red, green, blue, alpha = color
    if alpha != 217:
        return None
    if red == 201 and blue == 32 and 52 <= green <= 196:
        return 0
    if green == 32 and ((blue == 201 and 167 <= red <= 201) or (red == 201 and 54 <= blue <= 201)):
        return 1
    return None


def straight_alpha(image):
    # The compositor consumes premultiplied RGB, while PNG stores straight RGB.
    return Image.frombytes("RGBa", image.size, image.tobytes()).convert("RGBA")


def upload_ribbon_texture(tuned_wake):
    width = 256
    if tuned_wake:
        name, height, channels = "bfly-sunrise.rgba8", 768, 4
        internal_format, pixel_format = 0x8058, 0x1908  # GL_RGBA8, GL_RGBA
    else:
        name, height, channels = "bfly-twilight.r8", 256, 1
        internal_format, pixel_format = 0x8229, 0x1903  # GL_R8, GL_RED
    pixels = (ROOT / "shaders/trueos" / name).read_bytes()
    if len(pixels) != width * height * channels:
        raise RuntimeError(f"{name} needs a {width}×{height} texture with {channels} bytes per pixel")
    texture = create_buffer("GenTextures")
    gl("ActiveTexture", U)(0x84C0)  # GL_TEXTURE0
    gl("BindTexture", U, U)(TEXTURE_2D, texture)
    gl("TexStorage2D", U, I, U, I, I)(TEXTURE_2D, 1, internal_format, width, height)
    gl("TexSubImage2D", U, I, I, I, I, I, U, U, P)(
        TEXTURE_2D, 0, 0, 0, width, height, pixel_format, 0x1401, pixels)  # GL_UNSIGNED_BYTE
    for parameter, value in ((0x2801, 0x2601), (0x2800, 0x2601), (0x2802, 0x812F), (0x2803, 0x812F)):
        gl("TexParameteri", U, U, I)(TEXTURE_2D, parameter, value)
    check_gl(f"upload {name}")


def render_direct(args, size, controls, *, fill=False, layered=False, twilight=None, sunrise=False):
    tuned_wake = sunrise or twilight == "tuned"
    turn_limited = twilight not in ("short", "wide")
    original_controls = wing_control_points(direct=True)[0] if layered else []
    upload_controls(controls + original_controls, packed=True)
    camera = create_buffer("GenBuffers")
    gl("BindBuffer", U, U)(SHADER_STORAGE_BUFFER, camera)
    gl("BufferData", U, C.c_ssize_t, P, U)(SHADER_STORAGE_BUFFER, 368, None, DYNAMIC_DRAW)
    gl("BindBufferBase", U, U, U)(SHADER_STORAGE_BUFFER, 0, camera)
    # Resting positions are expressed in sixteenths of a tier.
    records = struct.pack(
        f"={BUTTERFLIES}I",
        *(0x20000000 | (seed << 22) | (position << 15) | (position << 8) | 255
          for seed, position in enumerate(range(0, BUTTERFLIES * 16, 16))),
    )
    update_camera = gl("BufferSubData", U, C.c_ssize_t, C.c_ssize_t, P)
    update_camera(SHADER_STORAGE_BUFFER, 0, len(records), records)
    stages = ((0x8B31, "vert"), (0x8E88, "tesc"), (0x8E87, "tese"), (0x8B30, "frag"))
    line_defines = (b"BFLY_MIXED_WINGS", b"BFLY_TIER_WIDTH")
    if twilight not in ("short", "wide"):
        line_defines += (b"BFLY_NEUTRAL_OUTLINE",)
    if tuned_wake:
        line_defines += (b"BFLY_INSET_OUTLINE", b"BFLY_SUNRISE")
    overlay_program = compile_program("direct-wings", stages, defines=line_defines,
                                      turn_limited=turn_limited) if layered else None
    ribbon_program = None
    ribbon_mode = 0.0
    if sunrise or twilight:
        upload_ribbon_texture(tuned_wake)
        ribbon_shader = "sunrise-ribbon" if tuned_wake else "twilight-ribbon"
        ribbon_defines = (b"BFLY_MIXED_WINGS",)
        if tuned_wake:
            ribbon_defines += (b"BFLY_INSET_OUTLINE", b"BFLY_SUNRISE")
        ribbon_mode = 1.0 if sunrise or twilight == "short" else 2.0
        ribbon_program = compile_program(
            "direct-wings", stages, defines=ribbon_defines + (b"BFLY_TWILIGHT_RIBBON",),
            stage_names={"tese": ribbon_shader, "frag": ribbon_shader}, turn_limited=turn_limited)
        gl("BlendFunc", U, U)(ONE, ONE_MINUS_SRC_ALPHA)
    fill_program = None
    if fill:
        line_defines += (b"BFLY_INSET_OUTLINE",)
        fill_program = compile_program("direct-wings", stages,
                                       defines=(b"BFLY_MIXED_WINGS", b"BFLY_FILL_WINGS",
                                                b"BFLY_INSET_FILL", b"BFLY_BREED_FILL",
                                                b"BFLY_TRANSLUCENT_FILL"),
                                       stage_names={"tese": "filled-wings", "frag": "filled-wings"},
                                       turn_limited=turn_limited)
    line_program = compile_program("direct-wings", stages, defines=line_defines, turn_limited=turn_limited)
    use_program = gl("UseProgram", U)
    use_program(line_program)
    gl("Viewport", I, I, I, I)(0, 0, *size)
    gl("ClearColor", F, F, F, F)(0, 0, 0, 0)
    gl("Disable", U)(BLEND)

    def frame(size, time, expected=None, query=None, *, first=0, count=len(controls)):
        framebuffer = (*size, time % FLAP_PHASE_PERIOD_SECONDS, FLIGHT_PERIOD_SECONDS)
        flock_motion = (time % FLOCK_PERIOD_SECONDS, 4.4, 4.4, BUTTERFLIES)
        swap_time = time % SWAP_PERIOD_SECONDS
        if tuned_wake:
            wake = (float(sunrise), float(not sunrise), 0.0)
        else:
            wake = (float(not fill and ribbon_program is None), 0.0, ribbon_mode)
        # Zero the eight unused records between framebuffer and flock_motion.
        data = struct.pack("=4f32x8f", *framebuffer, *flock_motion, swap_time, *wake)
        update_camera(SHADER_STORAGE_BUFFER, 288, len(data), data)
        gl("Clear", U)(COLOR_BUFFER_BIT)
        if fill_program is not None:
            use_program(fill_program)
            draw(PATCHES, count, 2 * BUTTERFLIES, first=first)
            use_program(line_program)
        if ribbon_program is not None:
            gl("Enable", U)(BLEND)
            use_program(ribbon_program)
            draw(PATCHES, count, 2 * BUTTERFLIES, first=first)
            gl("Disable", U)(BLEND)
            use_program(line_program)
        draw(PATCHES, count, 2 * BUTTERFLIES, expected, query, first=first, tier_widths=True)
        if overlay_program is not None:
            use_program(overlay_program)
            draw(PATCHES, len(original_controls), 2 * BUTTERFLIES, first=len(controls), tier_widths=True)
        return read_frame(size)

    if not fill:
        # Both halves of the startup flock contribute four hinges, one per butterfly.
        query = create_buffer("GenQueries")
        artwork_bit = 4 if tuned_wake else 2
        alternate = next(i for i, (_, _, tag) in enumerate(controls) if tag & artwork_bit)
        frame((1, 1), 0.0, BUTTERFLIES // 2, query, count=alternate)
        frame((1, 1), 0.0, BUTTERFLIES // 2, query, first=alternate, count=len(controls) - alternate)
        frame((1, 1), 0.0, BUTTERFLIES, query)
    first = frame(size, args.start)
    later = frame(size, args.start + 1 / FRAME_RATE)
    if size in ((1920, 1080), (1080, 1920), DEFAULT_SIZE, DEFAULT_SIZE[::-1]):
        transparent = (0, 0, 0, 0)
        white = (255, 255, 255, 255)
        non_fill = {transparent, white}
        palette_size = len(non_fill) + BUTTERFLIES
        if fill:
            rgba = {pixel for _, pixel in first.getcolors(palette_size) or ()}
            families = {fill_family(pixel) for pixel in rgba - non_fill}
            if not non_fill <= rgba or families != {0, 1}:
                raise RuntimeError("Expected translucent fill palettes and opaque white outlines over transparent pixels")
        elif ribbon_program is not None:
            alpha = first.getchannel("A").histogram()
            if not (alpha[0] and alpha[255] and sum(alpha[1:255])):
                raise RuntimeError("Expected translucent wake ribbons and opaque outlines over transparency")
        else:
            rgba = {pixel for _, pixel in first.getcolors(len(non_fill)) or ()}
            if rgba != non_fill:
                raise RuntimeError("Expected transparent pixels and opaque white lines in the direct framebuffer")
        if first.tobytes() == later.tobytes():
            raise RuntimeError("The direct flight shaders did not animate")
        initial = frame(size, 0.0)
        after_flap = frame(size, 4.0)
        if after_flap.tobytes() == initial.tobytes():
            raise RuntimeError("The butterfly pose did not advance after a complete wingbeat")
        if frame(size, MOTION_PERIOD_SECONDS).tobytes() != initial.tobytes():
            raise RuntimeError("The direct framebuffer changed on a full motion cycle")
        if fill:
            areas = {sum(count for count, pixel in (image.getcolors(palette_size) or ()) if pixel not in non_fill)
                     for image in (first, later, initial, after_flap)}
            if len(areas) < 2:
                raise RuntimeError("The projected fill area did not change during flight")
            print("Direct flight shaders rendered both 85% fill palettes beneath opaque white outlines")
        elif tuned_wake:
            sky = "Sunrise" if sunrise else "Twilight"
            print(f"Direct flight shaders rendered torn {sky} ribbons beneath closed white outlines")
        elif twilight:
            print(f"Direct flight shaders rendered the {twilight} Twilight ribbons beneath RGB outlines")
        else:
            print("Direct flight shaders rendered opaque white lines over transparent pixels")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    export = straight_alpha if fill or ribbon_program is not None else lambda image: image
    export(first).save(args.output.with_suffix(".png"))
    if args.frames > 1:
        animation = args.output.with_name(args.output.stem + "-animated.png")
        save_animation(animation, args.ffmpeg, size, args.frames, export(first), export(later),
                       lambda size, time: export(frame(size, args.start + time)))


def render(args):
    size = tuple(args.size)
    display, surface, handle = context(size, args.render_node)
    try:
        renderer = proc("glGetString", C.c_char_p, U)(0x1F01).decode()
        print(f"renderer={renderer}")
        layered = getattr(args, "layered", False)
        twilight = getattr(args, "twilight", None)
        sunrise = getattr(args, "sunrise", False)
        fill = getattr(args, "fill", False) or layered
        direct = args.direct or sunrise or bool(twilight)
        controls, bounds = wing_control_points(direct=direct, fill=fill, tuned_wake=sunrise or twilight == "tuned")
        if direct or fill:
            render_direct(args, size, controls, fill=fill, layered=layered, twilight=twilight, sunrise=sunrise)
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
        gl("Enable", U)(BLEND)
        gl("BlendFunc", U, U)(ONE, ONE_MINUS_SRC_ALPHA)
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
    parser.add_argument("--fill", action="store_true", help="draw the translucent insets with RGB outlines")
    parser.add_argument("--layered", action="store_true", help="add the original RGB outlines over the filled wings")
    parser.add_argument("--twilight", choices=("tuned", "short", "wide"),
                        help="draw the torn RGB Twilight wake or a legacy reference")
    parser.add_argument("--sunrise", action="store_true", help="draw the torn Sunrise motion ribbons")
    parser.add_argument("--start", type=float, default=0.0,
                        help="start time in seconds for a direct render")
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
    if args.twilight and (args.direct or args.fill or args.layered):
        parser.error("--twilight is a separate mode from --direct, --fill and --layered")
    if args.sunrise and (args.direct or args.fill or args.layered or args.twilight):
        parser.error("--sunrise is a separate mode from --direct, --fill, --layered and --twilight")
    if not isfinite(args.start) or args.start < 0 or (
        args.start != 0 and not (args.direct or args.fill or args.layered or args.twilight or args.sunrise)
    ):
        parser.error("--start needs a finite nonnegative time and a direct render mode")
    args.start %= MOTION_PERIOD_SECONDS
    if args.output is None:
        if args.sunrise:
            filename = "bfly-sunrise-wake.png"
        elif args.twilight:
            filename = f"bfly-twilight-{args.twilight}.png"
        elif args.layered:
            filename = "bfly-layered-wings.png"
        elif args.fill:
            filename = "bfly-filled-wings.png"
        else:
            filename = "bfly-direct-wings.png" if args.direct else "bfly-panels.png"
        args.output = ROOT / "target" / filename
    if args.frames > 1:
        args.ffmpeg = shutil.which("ffmpeg")
        if args.ffmpeg is None:
            parser.error("ffmpeg is required for animated captures")
    render(args)


if __name__ == "__main__":
    main()
