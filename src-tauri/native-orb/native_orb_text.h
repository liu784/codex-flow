#pragma once

#include <dwrite.h>
#include <d3d11.h>
#include <wrl/client.h>
#include <array>
#include <algorithm>
#include <string>
#include <vector>

// Immutable coverage atlas: all displayed values plus the unavailable marker.
// DirectWrite runs once during renderer creation; no desktop texture is read back.
inline HRESULT CreateOrbTextAtlas(ID3D11Device* device, ID3D11ShaderResourceView** view) {
    using Microsoft::WRL::ComPtr;
    constexpr UINT cellWidth = 64, cellHeight = 32, columns = 8, rows = 13;
    constexpr UINT width = cellWidth * columns, height = cellHeight * rows;
    ComPtr<IDWriteFactory> factory;
    HRESULT hr = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory),
                                    reinterpret_cast<IUnknown**>(factory.GetAddressOf()));
    if (FAILED(hr)) return hr;
    ComPtr<IDWriteFontCollection> fonts;
    hr = factory->GetSystemFontCollection(&fonts);
    if (FAILED(hr)) return hr;
    UINT32 familyIndex = 0;
    BOOL exists = FALSE;
    hr = fonts->FindFamilyName(L"Segoe UI", &familyIndex, &exists);
    if (FAILED(hr)) return hr;
    if (!exists) return HRESULT_FROM_WIN32(ERROR_NOT_FOUND);
    ComPtr<IDWriteFontFamily> family;
    hr = fonts->GetFontFamily(familyIndex, &family);
    if (FAILED(hr)) return hr;
    ComPtr<IDWriteFont> font;
    hr = family->GetFirstMatchingFont(DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_FONT_STRETCH_NORMAL,
                                     DWRITE_FONT_STYLE_NORMAL, &font);
    if (FAILED(hr)) return hr;
    ComPtr<IDWriteFontFace> face;
    hr = font->CreateFontFace(&face);
    if (FAILED(hr)) return hr;
    DWRITE_FONT_METRICS fontMetrics{};
    face->GetMetrics(&fontMetrics);
    const float scale = 20.0f / fontMetrics.designUnitsPerEm;
    std::vector<BYTE> atlas(width * height, 0);
    for (UINT value = 0; value < 102; ++value) {
        const std::wstring label = value == 101 ? L"--" : std::to_wstring(value) + L"%";
        const UINT32 count = static_cast<UINT32>(label.size());
        std::array<UINT32, 4> codepoints{};
        std::array<UINT16, 4> glyphs{};
        std::array<DWRITE_GLYPH_METRICS, 4> metrics{};
        std::array<float, 4> advances{};
        for (UINT32 i = 0; i < count; ++i) codepoints[i] = label[i];
        hr = face->GetGlyphIndices(codepoints.data(), count, glyphs.data());
        if (FAILED(hr)) return hr;
        hr = face->GetDesignGlyphMetrics(glyphs.data(), count, metrics.data(), FALSE);
        if (FAILED(hr)) return hr;
        float textWidth = 0.0f;
        for (UINT32 i = 0; i < count; ++i) {
            advances[i] = metrics[i].advanceWidth * scale;
            textWidth += advances[i];
        }
        DWRITE_GLYPH_RUN run{};
        run.fontFace = face.Get();
        run.fontEmSize = 20.0f;
        run.glyphCount = count;
        run.glyphIndices = glyphs.data();
        run.glyphAdvances = advances.data();
        ComPtr<IDWriteGlyphRunAnalysis> analysis;
        // Cap height centers the actual numeral ink, rather than the font line box.
        const float baseline = 16.0f + fontMetrics.capHeight * scale * 0.5f;
        hr = factory->CreateGlyphRunAnalysis(&run, 1.0f, nullptr,
            DWRITE_RENDERING_MODE_CLEARTYPE_NATURAL_SYMMETRIC, DWRITE_MEASURING_MODE_NATURAL,
            (64.0f - textWidth) * 0.5f, baseline, &analysis);
        if (FAILED(hr)) return hr;
        const RECT bounds{0, 0, cellWidth, cellHeight};
        std::vector<BYTE> coverage(cellWidth * cellHeight * 3, 0);
        hr = analysis->CreateAlphaTexture(DWRITE_TEXTURE_CLEARTYPE_3x1, &bounds,
                                         coverage.data(), static_cast<UINT32>(coverage.size()));
        if (FAILED(hr)) return hr;
        for (UINT y = 0; y < cellHeight; ++y) {
            for (UINT x = 0; x < cellWidth; ++x) {
                const UINT source = (y * cellWidth + x) * 3;
                // Grayscale coverage avoids RGB text fringes on transparent surfaces.
                const UINT alpha = (coverage[source] + coverage[source + 1] + coverage[source + 2]) / 3;
                atlas[((value / columns) * cellHeight + y) * width
                    + (value % columns) * cellWidth + x] = static_cast<BYTE>(alpha);
            }
        }
    }
    D3D11_TEXTURE2D_DESC description{};
    description.Width = width;
    description.Height = height;
    description.MipLevels = 1;
    description.ArraySize = 1;
    description.Format = DXGI_FORMAT_R8_UNORM;
    description.SampleDesc.Count = 1;
    description.Usage = D3D11_USAGE_IMMUTABLE;
    description.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    D3D11_SUBRESOURCE_DATA data{atlas.data(), width, 0};
    ComPtr<ID3D11Texture2D> texture;
    hr = device->CreateTexture2D(&description, &data, &texture);
    if (FAILED(hr)) return hr;
    return device->CreateShaderResourceView(texture.Get(), nullptr, view);
}
