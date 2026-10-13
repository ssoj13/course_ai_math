// C++ OpenJPH 0.31 counterpart of examples/tile_bench.rs: same exv-like streams, same protocol
// (pooled codestream + restart, interleaved per-line exchange/pull), single thread.
// Usage: tile_bench_cpp DIR [runs] [ms]   (DIR = `cargo run --release --example tile_bench -- --dump DIR`)
// Build (x64 Native Tools prompt, vcpkg openjph:x64-windows-static-release):
//   cl /O2 /MT /EHsc /std:c++17 /IC:\vcpkg\installed\x64-windows-static-release\include scripts\tile_bench.cpp
//      /Fe:tile_bench_cpp.exe /link C:\vcpkg\installed\x64-windows-static-release\lib\openjph.0.31.lib
// Known: OpenJPH 0.31 writes CAP Ccap15 = 0x0202 where jph-rs writes 0x0002 (1 byte, same length).
#include <openjph/ojph_codestream.h>
#include <openjph/ojph_file.h>
#include <openjph/ojph_mem.h>
#include <openjph/ojph_params.h>
#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <string>
#include <vector>

struct Case {
  std::string name; int w, h, nc, bits; bool sign;
  std::vector<uint8_t> j2c; std::vector<int32_t> raw;  // raw: line y of comp 0..nc, i32
};

static std::vector<uint8_t> slurp(const std::filesystem::path& p) {
  std::ifstream f(p, std::ios::binary);
  return std::vector<uint8_t>((std::istreambuf_iterator<char>(f)), {});
}

static void encode(ojph::codestream& cs, const Case& c, std::vector<uint8_t>& out) {
  cs.restart();
  ojph::param_siz siz = cs.access_siz();
  siz.set_num_components(c.nc);
  for (int i = 0; i < c.nc; ++i) {
    siz.set_component(i, ojph::point(1, 1), c.bits, c.sign);
    if (c.sign) cs.access_nlt().set_nonlinear_transform(i, 3);
  }
  cs.set_planar(false);
  siz.set_image_offset(ojph::point(0, 0));
  siz.set_image_extent(ojph::point(c.w, c.h));
  ojph::param_cod cod = cs.access_cod();
  cod.set_color_transform(false);
  cod.set_reversible(true);
  cod.set_block_dims(64, 64);
  cod.set_num_decomposition(5);
  ojph::mem_outfile of;
  of.open((size_t)c.w * c.h * c.nc * ((c.bits + 7) / 8) + 4096, false);
  cs.write_headers(&of);
  ojph::ui32 next = 0;
  ojph::line_buf* cur = cs.exchange(nullptr, next);
  for (int y = 0; y < c.h; ++y)
    for (int ci = 0; ci < c.nc; ++ci) {
      const int32_t* src = c.raw.data() + ((size_t)y * c.nc + ci) * c.w;
      if (cur->flags & ojph::line_buf::LFT_32BIT) memcpy(cur->i32, src, c.w * 4);
      else for (int x = 0; x < c.w; ++x) cur->i64[x] = src[x];
      cur = cs.exchange(cur, next);
    }
  cs.flush();
  out.assign(of.get_data(), of.get_data() + of.tell());
}

static void decode(ojph::codestream& cs, const Case& c, std::vector<int32_t>& planes) {
  cs.restart();
  ojph::mem_infile inf;
  inf.open(c.j2c.data(), c.j2c.size());
  cs.read_headers(&inf);
  cs.set_planar(false);
  cs.create();
  ojph::ui32 comp = 0;
  for (int y = 0; y < c.h; ++y)
    for (int ci = 0; ci < c.nc; ++ci) {
      ojph::line_buf* l = cs.pull(comp);
      int32_t* dst = planes.data() + ((size_t)y * c.nc + ci) * c.w;
      if (l->flags & ojph::line_buf::LFT_32BIT) memcpy(dst, l->i32, c.w * 4);
      else for (int x = 0; x < c.w; ++x) dst[x] = (int32_t)l->i64[x];
    }
  inf.close();
}

template <class F> static double per_call(int reps, F f) {
  auto t = std::chrono::steady_clock::now();
  for (int i = 0; i < reps; ++i) f();
  return std::chrono::duration<double>(std::chrono::steady_clock::now() - t).count() / reps;
}

int main(int argc, char** argv) {
  std::string dir = argc > 1 ? argv[1] : "C:/temp/jph-dump";
  int runs = argc > 2 ? atoi(argv[2]) : 7;
  double ms = argc > 3 ? atof(argv[3]) : 40.0;
  std::vector<Case> cases;
  for (auto& e : std::filesystem::directory_iterator(dir)) {
    if (e.path().extension() != ".j2c") continue;
    Case c; c.name = e.path().stem().string();
    int s, nc; char ty[8] = {};
    sscanf(c.name.c_str(), "%dx%dc_%7[^_]", &s, &nc, ty);
    c.w = c.h = s; c.nc = nc;
    std::string t = ty;
    c.bits = t == "u8" ? 8 : (t == "u16" || t == "f16") ? 16 : 32;
    c.sign = t == "f16" || t == "f32";
    c.j2c = slurp(e.path());
    auto r = slurp(std::filesystem::path(e.path()).replace_extension(".raw"));
    c.raw.resize(r.size() / 4); memcpy(c.raw.data(), r.data(), r.size());
    cases.push_back(std::move(c));
  }
  ojph::codestream cs;
  std::vector<std::vector<double>> dec(cases.size()), enc(cases.size());
  std::vector<std::pair<int, int>> reps;
  std::vector<uint8_t> out;
  for (auto& c : cases) {
    std::vector<int32_t> planes(c.raw.size());
    decode(cs, c, planes);
    if (planes != c.raw) printf("MISMATCH decode %s\n", c.name.c_str());
    encode(cs, c, out);
    if (out != c.j2c) { std::ofstream(dir + "/" + c.name + ".cpp.j2c", std::ios::binary).write((const char*)out.data(), out.size()); size_t k = 0; while (k < out.size() && out[k] == c.j2c[k]) ++k; printf("first diff at byte %zu\n", k); } if (out != c.j2c) printf("NOTE: C++ codestream differs from Rust for %s (%zu vs %zu B)\n", c.name.c_str(), out.size(), c.j2c.size());
    double d = per_call(2, [&] { decode(cs, c, planes); }), en = per_call(2, [&] { encode(cs, c, out); });
    reps.push_back({std::max(1, (int)(ms / 1000 / d)), std::max(1, (int)(ms / 1000 / en))});
  }
  for (int r = 0; r < runs; ++r)
    for (size_t i = 0; i < cases.size(); ++i) {
      std::vector<int32_t> planes(cases[i].raw.size());
      dec[i].push_back(per_call(reps[i].first, [&] { decode(cs, cases[i], planes); }));
      enc[i].push_back(per_call(reps[i].second, [&] { encode(cs, cases[i], out); }));
    }
  printf("%-26s %9s %9s %9s %9s\n", "case", "dec ms", "dec MB/s", "enc ms", "enc MB/s");
  for (size_t i = 0; i < cases.size(); ++i) {
    std::sort(dec[i].begin(), dec[i].end()); std::sort(enc[i].begin(), enc[i].end());
    double d = dec[i][runs / 2], e = enc[i][runs / 2];
    double mb = (double)cases[i].w * cases[i].h * cases[i].nc * ((cases[i].bits + 7) / 8) / 1e6;
    printf("%-26s %9.3f %9.1f %9.3f %9.1f\n", cases[i].name.c_str(), d * 1e3, mb / d, e * 1e3, mb / e);
  }
}
