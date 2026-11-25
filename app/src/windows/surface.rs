//! A window's drawing surface: a Direct2D device context rendering into a
//! premultiplied-alpha DXGI swap chain that DirectComposition places in the
//! window. Transparent pixels show the system backdrop (acrylic) beneath.

use super::painter;
use std::cell::RefCell;
use windows::core::Interface;
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectComposition::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;

/// The GPU device shared by every window.
struct Gpu {
    dxgi: IDXGIDevice,
    d2d: ID2D1Device,
    factory: IDXGIFactory2,
}

thread_local! {
    static GPU: RefCell<Option<Gpu>> = const { RefCell::new(None) };
}

fn create_gpu() -> windows::core::Result<Gpu> {
    let mut device = None;
    let flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;
    unsafe {
        // Hardware first; WARP (software) when there's no usable GPU.
        let hw = D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), flags, None, D3D11_SDK_VERSION, Some(&mut device), None, None);
        if hw.is_err() {
            D3D11CreateDevice(None, D3D_DRIVER_TYPE_WARP, HMODULE::default(), flags, None, D3D11_SDK_VERSION, Some(&mut device), None, None)?;
        }
    }
    let device: ID3D11Device = device.ok_or_else(windows::core::Error::empty)?;
    let dxgi: IDXGIDevice = device.cast()?;
    let d2d = unsafe { painter::d2d().CreateDevice(&dxgi) }?;
    let factory: IDXGIFactory2 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }?;
    Ok(Gpu { dxgi, d2d, factory })
}

fn with_gpu<R>(f: impl FnOnce(&Gpu) -> windows::core::Result<R>) -> windows::core::Result<R> {
    GPU.with(|g| {
        let mut g = g.borrow_mut();
        if g.is_none() {
            *g = Some(create_gpu()?);
        }
        f(g.as_ref().unwrap())
    })
}

/// Forget the device after it was lost; the next surface makes a new one.
pub fn reset_device() {
    GPU.with(|g| *g.borrow_mut() = None);
}

pub struct Surface {
    pub dc: ID2D1DeviceContext,
    swap: IDXGISwapChain1,
    _dcomp: IDCompositionDevice,
    _target: IDCompositionTarget,
    _visual: IDCompositionVisual,
    size: (u32, u32),
    pub shadows: painter::ShadowCache,
}

impl Surface {
    pub fn new(hwnd: HWND, width: u32, height: u32) -> windows::core::Result<Surface> {
        let (width, height) = (width.max(1), height.max(1));
        with_gpu(|gpu| unsafe {
            let dc = gpu.d2d.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: width,
                Height: height,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                ..Default::default()
            };
            let swap = gpu.factory.CreateSwapChainForComposition(&gpu.dxgi, &desc, None)?;
            let dcomp: IDCompositionDevice = DCompositionCreateDevice(&gpu.dxgi)?;
            let target = dcomp.CreateTargetForHwnd(hwnd, true)?;
            let visual = dcomp.CreateVisual()?;
            visual.SetContent(&swap)?;
            target.SetRoot(&visual)?;
            dcomp.Commit()?;
            Ok(Surface { dc, swap, _dcomp: dcomp, _target: target, _visual: visual, size: (width, height), shadows: painter::ShadowCache::new() })
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) -> windows::core::Result<()> {
        let (width, height) = (width.max(1), height.max(1));
        if self.size == (width, height) {
            return Ok(());
        }
        unsafe {
            self.dc.SetTarget(None);
            self.swap.ResizeBuffers(0, width, height, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))?;
        }
        self.size = (width, height);
        Ok(())
    }

    /// Points the device context at the back buffer and begins drawing.
    pub fn begin(&self, dpi: f32) -> windows::core::Result<()> {
        unsafe {
            let buffer: IDXGISurface = self.swap.GetBuffer(0)?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: dpi,
                dpiY: dpi,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                ..Default::default()
            };
            let bitmap = self.dc.CreateBitmapFromDxgiSurface(&buffer, Some(&props))?;
            self.dc.SetTarget(&bitmap);
            self.dc.SetDpi(dpi, dpi);
            self.dc.BeginDraw();
        }
        Ok(())
    }

    /// Ends drawing and shows the frame.
    pub fn end(&self) -> windows::core::Result<()> {
        unsafe {
            let drawn = self.dc.EndDraw(None, None);
            self.dc.SetTarget(None);
            drawn?;
            self.swap.Present(1, DXGI_PRESENT(0)).ok()
        }
    }
}
