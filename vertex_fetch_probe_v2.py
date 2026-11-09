"""Windows OpenGL vertex-fetch probe v2 for horizon-rust/horizon#6447.

Standard-library only. Run outside RenderDoc first; writes JSON to --output.
This tests vertex fetch, not Horizon's complete rendering pipeline.
"""
import argparse
import ctypes as C
from ctypes import wintypes as W
import json
from pathlib import Path
import struct
import sys


LAYERS = (0, 1, 1215, 32767, 32768, 33983, 65535)
BASE = 37808
INDEX_OFFSET = 9984
VERTEX = struct.Struct('<3f2HHHI')
KINDS = ('u16_scalar', 'u16x2_scalar', 'u16x2_vector', 'u32_packed',
         'u16x2_shared', 'u32_shared', 'u8x4_shared', 'light_u8x4',
         'light_u16', 'light_u16x2')


def fixtures():
    rows = [(layer, i * 4096, 32768 - i * 1024, 0x10 + i * 0x2010)
            for i, layer in enumerate(LAYERS)]
    data = b''.join(VERTEX.pack(0., 0., 0., u, v, layer, light, 0xFFFFFFFF)
                    for layer, u, v, light in rows)
    assert VERTEX.size == 24
    assert struct.unpack_from('<H', data, 2 * 24 + 16)[0] == 1215
    return rows, data


def fields(actual, expected):
    return {name: len(actual) == len(expected) and all(a[i] == e[i]
            for a, e in zip(actual, expected))
            for i, name in enumerate(('layer', 'uv_u', 'uv_v', 'light'))}


def inputs(kind, layer_offset=16, swap=False):
    """Return attribute descriptions and GLSL expressions for the unchanged bytes."""
    layer_location, uv_location = (1, 2) if swap else (2, 1)
    size, gltype, typename = 1, 0x1403, 'uint'
    layer, light = 'layer', 'light.x | (light.y << 8u)'
    if kind.startswith('u16x2'):
        size = 2
        if kind != 'u16x2_scalar':
            typename, layer = 'uvec2', 'layer.x'
        if kind == 'u16x2_shared':
            light = 'layer.y'
    elif kind.startswith('u32') or kind.startswith('light_'):
        gltype, layer = 0x1405, '(layer & 65535u)'
        if kind == 'u32_shared':
            light = '(layer >> 16u)'
    elif kind == 'u8x4_shared':
        size, gltype, typename = 4, 0x1401, 'uvec4'
        layer, light = '(layer.x | (layer.y << 8u))', '(layer.z | (layer.w << 8u))'
    attributes = [(0, 3, 0x1406, 0, False), (uv_location, 2, 0x1403, 12, True),
                  (layer_location, size, gltype, layer_offset, True),
                  (3, 4, 0x1401, 20, False)]
    declarations = ('layout(location=0) in vec3 position;\n'
                    'layout(location=3) in vec4 color;\n'
                    f'layout(location={uv_location}) in uvec2 uv;\n'
                    f'layout(location={layer_location}) in {typename} layer;\n')
    if not kind.endswith('_shared'):
        size, gltype, typename = 2, 0x1401, 'uvec2'
        if kind == 'light_u8x4':
            size, typename = 4, 'uvec4'
        elif kind == 'light_u16':
            size, gltype, typename, light = 1, 0x1403, 'uint', 'light'
        elif kind == 'light_u16x2':
            gltype, light = 0x1403, 'light.x'
        attributes.append((4, size, gltype, 18, True))
        declarations += f'layout(location=4) in {typename} light;\n'
    return attributes, declarations, layer, light


class Context:
    def __enter__(self):
        self.user = C.WinDLL('user32', use_last_error=True)
        self.gdi = C.WinDLL('gdi32', use_last_error=True)
        self.dll = C.WinDLL('opengl32', use_last_error=True)
        self.window = self.dc = self.rc = None
        self.user.CreateWindowExW.argtypes = [W.DWORD, W.LPCWSTR, W.LPCWSTR, W.DWORD,
            C.c_int, C.c_int, C.c_int, C.c_int, W.HWND, W.HMENU, W.HINSTANCE, W.LPVOID]
        self.user.CreateWindowExW.restype = W.HWND
        self.user.GetDC.argtypes, self.user.GetDC.restype = [W.HWND], W.HDC
        self.user.ReleaseDC.argtypes = [W.HWND, W.HDC]
        self.user.DestroyWindow.argtypes = [W.HWND]
        self.dll.wglCreateContext.argtypes = [W.HDC]
        self.dll.wglCreateContext.restype = W.HANDLE
        self.dll.wglMakeCurrent.argtypes = [W.HDC, W.HANDLE]
        self.dll.wglDeleteContext.argtypes = [W.HANDLE]
        self.dll.wglGetProcAddress.argtypes = [C.c_char_p]
        self.dll.wglGetProcAddress.restype = C.c_void_p
        self.gdi.ChoosePixelFormat.argtypes = [W.HDC, C.c_void_p]
        self.gdi.SetPixelFormat.argtypes = [W.HDC, C.c_int, C.c_void_p]
        try:
            # STATIC is a built-in window class; no visible window or message loop needed.
            self.window = self.user.CreateWindowExW(0, 'STATIC', 'Vertex fetch probe',
                                                    0, 0, 0, 32, 32, None, None, None, None)
            if not self.window:
                raise C.WinError(C.get_last_error())
            self.dc = self.user.GetDC(self.window)
            # PIXELFORMATDESCRIPTOR, 40-byte Windows ABI: DRAW_TO_WINDOW|SUPPORT_OPENGL.
            pfd = C.create_string_buffer(40)
            struct.pack_into('<HHIBB', pfd, 0, 40, 1, 0x24, 0, 24)
            fmt = self.gdi.ChoosePixelFormat(self.dc, pfd)
            if not fmt or not self.gdi.SetPixelFormat(self.dc, fmt, pfd):
                raise C.WinError(C.get_last_error())
            self.rc = self.dll.wglCreateContext(self.dc)
            if not self.rc or not self.dll.wglMakeCurrent(self.dc, self.rc):
                raise RuntimeError('Cannot create bootstrap OpenGL context')
            create = self.fn('wglCreateContextAttribsARB', W.HANDLE,
                             W.HDC, W.HANDLE, C.POINTER(C.c_int))
            attrs = (C.c_int * 7)(0x2091, 4, 0x2092, 3, 0x9126, 1, 0)
            modern = create(self.dc, None, attrs)
            if not modern:
                raise RuntimeError('OpenGL 4.3 core context unavailable on this adapter')
            self.dll.wglMakeCurrent(None, None)
            self.dll.wglDeleteContext(self.rc)
            self.rc = modern
            if not self.dll.wglMakeCurrent(self.dc, self.rc):
                raise RuntimeError('Cannot activate OpenGL 4.3 context')
            return self
        except BaseException:
            self.__exit__(None, None, None)
            raise

    def fn(self, name, result, *args):
        address = self.dll.wglGetProcAddress(name.encode())
        if address in (None, 1, 2, 3, C.c_void_p(-1).value):
            address = C.cast(getattr(self.dll, name), C.c_void_p).value
        return C.WINFUNCTYPE(result, *args)(address)

    def __exit__(self, *_):
        if self.rc:
            self.dll.wglMakeCurrent(None, None)
            self.dll.wglDeleteContext(self.rc)
        if self.dc:
            self.user.ReleaseDC(self.window, self.dc)
        if self.window:
            self.user.DestroyWindow(self.window)


class Probe:
    def __init__(self, ctx):
        U, I, P = C.c_uint, C.c_int, C.c_void_p
        signatures = {
            'GetString': (C.c_char_p, U), 'GetError': (U,),
            'GenVertexArrays': (None, I, P), 'BindVertexArray': (None, U),
            'DeleteVertexArrays': (None, I, P),
            'GenBuffers': (None, I, P), 'BindBuffer': (None, U, U),
            'DeleteBuffers': (None, I, P),
            'BufferData': (None, U, C.c_ssize_t, P, U),
            'BufferSubData': (None, U, C.c_ssize_t, C.c_ssize_t, P),
            'GetBufferSubData': (None, U, C.c_ssize_t, C.c_ssize_t, P),
            'BindBufferBase': (None, U, U, U),
            'CreateShader': (U, U), 'ShaderSource': (None, U, I, P, P),
            'CompileShader': (None, U), 'GetShaderiv': (None, U, U, P),
            'GetShaderInfoLog': (None, U, I, P, P), 'DeleteShader': (None, U),
            'CreateProgram': (U,), 'AttachShader': (None, U, U),
            'TransformFeedbackVaryings': (None, U, I, P, U),
            'LinkProgram': (None, U), 'GetProgramiv': (None, U, U, P),
            'GetProgramInfoLog': (None, U, I, P, P),
            'UseProgram': (None, U), 'DeleteProgram': (None, U),
            'EnableVertexAttribArray': (None, U),
            'VertexAttribIPointer': (None, U, I, U, I, P),
            'VertexAttribIFormat': (None, U, I, U, U),
            'VertexAttribPointer': (None, U, I, U, C.c_ubyte, I, P),
            'VertexAttribFormat': (None, U, I, U, C.c_ubyte, U),
            'VertexAttribBinding': (None, U, U),
            'BindVertexBuffer': (None, U, U, C.c_ssize_t, I),
            'Enable': (None, U), 'Disable': (None, U),
            'BeginTransformFeedback': (None, U), 'EndTransformFeedback': (None,),
            'DrawArrays': (None, U, I, I),
            'DrawElementsBaseVertex': (None, U, I, U, P, I),
            'GenQueries': (None, I, P), 'BeginQuery': (None, U, U),
            'EndQuery': (None, U), 'GetQueryObjectuiv': (None, U, U, P),
            'DeleteQueries': (None, I, P), 'Finish': (None,),
            'GetVertexAttribiv': (None, U, U, P),
            'GenFramebuffers': (None, I, P), 'BindFramebuffer': (None, U, U),
            'DeleteFramebuffers': (None, I, P),
            'CheckFramebufferStatus': (U, U),
            'FramebufferTexture2D': (None, U, U, U, U, I),
            'GenTextures': (None, I, P), 'BindTexture': (None, U, U),
            'TexImage2D': (None, U, I, I, I, I, I, U, U, P),
            'DeleteTextures': (None, I, P),
            'Viewport': (None, I, I, I, I), 'PointSize': (None, C.c_float),
            'ReadPixels': (None, I, I, I, I, U, U, P),
            'ClearBufferuiv': (None, U, I, P),
        }
        for name, signature in signatures.items():
            setattr(self, name, ctx.fn('gl' + name, *signature))

    def check(self):
        error = self.GetError()
        if error:
            raise RuntimeError(f'OpenGL error 0x{error:04x}')

    def program(self, kind, mode, base, constant, swap):
        _, declarations, layer, light = inputs(kind, swap=swap)
        value = f'uvec4({layer}, uv, {light})'
        if constant:
            declarations, value = '', 'uvec4(1215u, 4096u, 32768u, 4660u)'
        split = mode == 'tf_split'
        outputs = ('flat out uint layer_out;\nflat out uvec3 controls;' if split
                   else 'flat out uvec4 result;')
        assign = ('layer_out = value.x; controls = value.yzw;' if split
                  else 'result = value;')
        source = f'''#version 430 core
{declarations}
{outputs}
void main() {{
    uvec4 value = {value};
    {assign}
    float x = (2.0 * float(gl_VertexID - {base}) + 1.0) / {len(LAYERS)}.0 - 1.0;
    gl_Position = {('vec4(x, 0.0, 0.0, 1.0)' if constant else
                     'vec4(x + position.x, position.y, position.z, color.a)')};
}}
'''
        program = self.CreateProgram()
        try:
            stages = [(0x8B31, source)]
            if mode == 'pixels':
                stages.append((0x8B30, '''#version 430 core
flat in uvec4 result;
layout(location=0) out uvec4 pixel;
void main() { pixel = result; }
'''))
            for stage, text in stages:
                shader = self.CreateShader(stage)
                try:
                    pointer = C.c_char_p(text.encode())
                    self.ShaderSource(shader, 1, C.byref(pointer), None)
                    self.CompileShader(shader)
                    ok = C.c_int()
                    self.GetShaderiv(shader, 0x8B81, C.byref(ok))
                    if not ok.value:
                        log = C.create_string_buffer(8192)
                        self.GetShaderInfoLog(shader, len(log), None, log)
                        raise RuntimeError(log.value.decode(errors='replace'))
                    self.AttachShader(program, shader)
                finally:
                    self.DeleteShader(shader)
            if mode != 'pixels':
                names = [b'layer_out', b'controls'] if split else [b'result']
                varyings = (C.c_char_p * len(names))(*names)
                self.TransformFeedbackVaryings(program, len(names), varyings, 0x8C8C)
            self.LinkProgram(program)
            ok = C.c_int()
            self.GetProgramiv(program, 0x8B82, C.byref(ok))
            if not ok.value:
                log = C.create_string_buffer(8192)
                self.GetProgramInfoLog(program, len(log), None, log)
                raise RuntimeError(log.value.decode(errors='replace'))
            return program
        except BaseException:
            self.DeleteProgram(program)
            raise

    def run_case(self, api, kind, indexed=False, mode='tf_vector', base=BASE,
                 constant=False, layer_offset=16, swap=False):
        expected, vertices = fixtures()
        count = len(expected)
        vao, buffers, query = C.c_uint(), (C.c_uint * 3)(), C.c_uint()
        if constant:
            expected = [(1215, 4096, 32768, 4660)] * count
        program = self.program(kind, mode, base, constant, swap)
        framebuffer, texture = C.c_uint(), C.c_uint()
        self.GenVertexArrays(1, C.byref(vao))
        self.GenBuffers(3, buffers)
        self.GenQueries(1, C.byref(query))
        try:
            self.BindVertexArray(vao)
            self.BindBuffer(0x8892, buffers[0])
            # Retain the report's nonzero base vertex in both draw variants.
            self.BufferData(0x8892, base * 24 + len(vertices), None, 0x88E4)
            self.BufferSubData(0x8892, base * 24, len(vertices), vertices)
            uploaded = C.create_string_buffer(len(vertices))
            self.GetBufferSubData(0x8892, base * 24, len(vertices), uploaded)
            upload_ok = uploaded.raw == vertices
            attributes, _, _, _ = inputs(kind, layer_offset, swap)
            if constant:
                attributes = []
            if api == 'separate':
                # wgpu unbinds ARRAY_BUFFER before its separate-format path.
                self.BindBuffer(0x8892, 0)
            state = []
            for location, size, gltype, offset, integer in attributes:
                self.EnableVertexAttribArray(location)
                if api == 'separate':
                    if integer:
                        self.VertexAttribIFormat(location, size, gltype, offset)
                    else:
                        self.VertexAttribFormat(location, size, gltype, 1, offset)
                    self.VertexAttribBinding(location, 0)
                elif integer:
                    self.VertexAttribIPointer(location, size, gltype, 24, offset)
                else:
                    self.VertexAttribPointer(location, size, gltype, 1, 24, offset)
                observed = {}
                for name, key in [('enabled', 0x8622), ('size', 0x8623),
                                  ('type', 0x8625), ('integer', 0x88FD),
                                  ('binding', 0x82D4), ('relative_offset', 0x82D5)]:
                    result = C.c_int()
                    self.GetVertexAttribiv(location, key, C.byref(result))
                    observed[name] = result.value
                state.append(dict(location=location, requested_size=size,
                                  requested_type=gltype, requested_offset=offset,
                                  observed=observed))
            if api == 'separate':
                self.BindVertexBuffer(0, buffers[0], 0, 24)
            if indexed:
                indices = struct.pack('<' + 'I' * count, *range(count))
                self.BindBuffer(0x8893, buffers[1])
                self.BufferData(0x8893, INDEX_OFFSET * 4 + len(indices), None, 0x88E4)
                self.BufferSubData(0x8893, INDEX_OFFSET * 4, len(indices), indices)
            if mode == 'pixels':
                self.GenFramebuffers(1, C.byref(framebuffer))
                self.GenTextures(1, C.byref(texture))
                self.BindTexture(0x0DE1, texture)
                self.TexImage2D(0x0DE1, 0, 0x8D70, count, 1, 0, 0x8D99, 0x1405, None)
                self.BindFramebuffer(0x8D40, framebuffer)
                self.FramebufferTexture2D(0x8D40, 0x8CE0, 0x0DE1, texture, 0)
                status = self.CheckFramebufferStatus(0x8D40)
                if status != 0x8CD5:
                    raise RuntimeError(f'Integer framebuffer incomplete: 0x{status:x}')
                sentinel = (C.c_uint * 4)(*[0xCDCDCDCD] * 4)
                self.ClearBufferuiv(0x1800, 0, sentinel)
                self.Viewport(0, 0, count, 1)
                self.PointSize(1.0)
            else:
                self.BindBuffer(0x8C8E, buffers[2])
                sentinel = bytes([0xCD]) * (count * 16)
                self.BufferData(0x8C8E, len(sentinel), sentinel, 0x88E1)
                self.BindBufferBase(0x8C8E, 0, buffers[2])
                self.Enable(0x8C89)  # RASTERIZER_DISCARD
            self.UseProgram(program)
            self.check()
            if mode != 'pixels':
                self.BeginQuery(0x8C88, query)
                self.BeginTransformFeedback(0)  # POINTS
            if indexed:
                self.DrawElementsBaseVertex(0, count, 0x1405, INDEX_OFFSET * 4, base)
            else:
                self.DrawArrays(0, base, count)
            if mode != 'pixels':
                self.EndTransformFeedback()
                self.EndQuery(0x8C88)
            self.Disable(0x8C89)
            self.Finish()
            self.check()
            written = C.c_uint()
            raw = C.create_string_buffer(count * 16)
            if mode == 'pixels':
                self.ReadPixels(0, 0, count, 1, 0x8D99, 0x1405, raw)
            else:
                self.GetQueryObjectuiv(query, 0x8866, C.byref(written))
                self.GetBufferSubData(0x8C8E, 0, count * 16, raw)
            self.check()
            actual = list(struct.iter_unpack('<4I', raw.raw))
            return dict(api=api, format=kind, draw='indexed_base_vertex' if indexed else 'arrays',
                        mode=mode, base_vertex=base, constant=constant,
                        layer_offset=layer_offset, swap_locations=swap,
                        passed=actual == expected and upload_ok and
                            (mode == 'pixels' or written.value == count),
                        fields=fields(actual, expected), input_buffer_readback_ok=upload_ok,
                        attribute_state=state,
                        primitives_written=None if mode == 'pixels' else written.value,
                        expected=expected, actual=actual)
        finally:
            self.UseProgram(0)
            self.Disable(0x8C89)
            self.BindFramebuffer(0x8D40, 0)
            self.DeleteFramebuffers(1, C.byref(framebuffer))
            self.DeleteTextures(1, C.byref(texture))
            self.BindVertexArray(0)
            self.BindBufferBase(0x8C8E, 0, 0)
            self.DeleteProgram(program)
            self.DeleteQueries(1, C.byref(query))
            self.DeleteBuffers(3, buffers)
            self.DeleteVertexArrays(1, C.byref(vao))


def self_test():
    expected, data = fixtures()
    for i, row in enumerate(expected):
        packed = struct.unpack_from('<I', data, i * 24 + 16)[0]
        assert (packed & 65535, packed >> 16) == (row[0], row[3])
        low, high, sky, block = struct.unpack_from('<4B', data, i * 24 + 16)
        assert (low | high << 8, sky | block << 8) == (row[0], row[3])
    assert all(fields(expected, expected).values())
    wrong = [(0, *row[1:3], 0) for row in expected]
    assert fields(wrong, expected) == dict(layer=False, uv_u=True, uv_v=True, light=False)
    light_only = [(*row[:3], 0) for row in expected]
    assert fields(light_only, expected) == dict(layer=True, uv_u=True, uv_v=True, light=False)
    for kind in KINDS:
        attrs, _, _, _ = inputs(kind)
        for _, size, gltype, offset, _ in attrs:
            width = {0x1401: 1, 0x1403: 2, 0x1405: 4, 0x1406: 4}[gltype]
            assert offset + size * width <= 24
    print('PASS: fixtures, packed decoding, per-field verdicts and attribute bounds')


def plan():
    for mode in ('tf_split', 'tf_vector', 'pixels'):
        yield dict(api='separate', kind='u16_scalar', mode=mode, constant=True)
    for api in ('separate', 'pointer'):
        for kind in KINDS:
            for indexed in (False, True):
                yield dict(api=api, kind=kind, indexed=indexed)
        for kind in ('u16_scalar', 'u32_shared'):
            for mode in ('tf_split', 'pixels'):
                yield dict(api=api, kind=kind, mode=mode)
            yield dict(api=api, kind=kind, base=0)
            yield dict(api=api, kind=kind, swap=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=Path('results-native-v2.json'))
    parser.add_argument('--self-test', action='store_true', help='check CPU fixture only; no GPU')
    args = parser.parse_args()
    self_test()
    if args.self_test:
        return 0
    if sys.platform != 'win32':
        parser.error('GPU probe requires Windows; --self-test works on other platforms')
    report = dict(probe_version=2,
                  issue='https://github.com/horizon-rust/horizon/issues/6447',
                  scope='Raw GL vertex fetch, transform feedback and integer pixels; not Horizon',
                  cases=[], negative_controls=[])
    try:
        with Context() as ctx:
            probe = Probe(ctx)
            report['adapter'] = {name: probe.GetString(key).decode(errors='replace')
                                 for name, key in [('vendor', 0x1F00), ('renderer', 0x1F01),
                                                   ('version', 0x1F02), ('glsl', 0x8B8C)]}
            print(json.dumps(report['adapter'], indent=2))
            for options in plan():
                try:
                    case = probe.run_case(**options)
                    print(f"{'PASS' if case['passed'] else 'FAIL'}: {options} fields={case['fields']}")
                except Exception as error:
                    case = dict(options=options, passed=False, error=str(error))
                    print(f'ERROR: {options}: {error}')
                    while probe.GetError():
                        pass
                report['cases'].append(case)
            for api in ('separate', 'pointer'):
                for mode in ('tf_vector', 'pixels'):
                    options = dict(api=api, kind='u32_shared', mode=mode, layer_offset=12)
                    try:
                        negative = probe.run_case(**options)
                        wrong = [(e[1], e[1], e[2], e[2]) for e in negative['expected']]
                        negative['expected_wrong_offset'] = wrong
                        negative['detected'] = (negative['actual'] == wrong and
                            negative['input_buffer_readback_ok'] and
                            (mode == 'pixels' or negative['primitives_written'] == len(wrong)))
                        negative['status'] = 'detected' if negative['detected'] else 'inconclusive'
                        print(f"CONTROL: {api} {mode} wrong_offset={negative['status']}")
                    except Exception as error:
                        negative = dict(options=options, detected=False, status='error', error=str(error))
                        while probe.GetError():
                            pass
                    report['negative_controls'].append(negative)
    except Exception as error:
        report['error'] = str(error)
    args.output.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(f'Results: {args.output.resolve()}')
    if 'error' in report:
        print(report['error'], file=sys.stderr)
        return 2
    if any('error' in case for case in report['cases'] + report['negative_controls']):
        return 2
    return 0 if (all(case['passed'] for case in report['cases']) and
                 all(case['detected'] for case in report['negative_controls'])) else 1


if __name__ == '__main__':
    raise SystemExit(main())
