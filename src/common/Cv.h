// In-house computer vision (equivalent of TSCV.*, ds_arch §11 / ds_pseudocode §9)
#pragma once
#include "Image.h"

namespace snap::cv {

using Mask = std::vector<uint8_t>;   // w*h, 0 or 255

// §9.1 WandSelector::Select — region growing ordered by cost (bounded Dijkstra)
//   tolerance ∈ (0, 100]; global = GlobalWandSelector (all pixels of a similar color)
Mask WandSelect(const Image& img, POINT seed, float tolerance, bool global);
// §10.2 DoWand: clamp [0.01, 100] and shortcut 100 → whole layer
Mask DoWand(const Image& img, POINT seed, float tolerance, bool global);

// helper masks
Mask RectMask(int w, int h, RECT r);
Mask EllipseMask(int w, int h, RECT r);
Mask PolygonMask(int w, int h, const std::vector<POINT>& pts);
RECT MaskBounds(const Mask& m, int w, int h);
bool MaskEmpty(const Mask& m);
Mask Dilate(const Mask& m, int w, int h, int k);

// §9.4 Dingo7Inpainter — paints the selection with the dominant color of the dilated neighborhood
bool Dingo7Inpaint(Image& img, const Mask& mask, int kernel, uint32_t* dominantOut = nullptr, double* shareOut = nullptr);
// Diffusion (role of NavierStokesInpainter / cv::inpaint): fills by Laplace iteration starting from the border
void DiffusionInpaint(Image& img, const Mask& mask, int iterations = 0);
// Smart fill: Dingo7 when the neighborhood is nearly uniform; diffusion otherwise
void SmartFill(Image& img, const Mask& mask);

// §9.6 Smart Move automatic tolerance
float SmartMoveAutoTolerance(const Image& img);
float SmartMoveToleranceScaling(float t, float inMin, float inMax, float outMin, float outMax);

// §9.5 PhaseCorrelationTranslationDetector — offset (dx,dy) of b relative to a
//   returns the phase correlation peak; confidence = relative height of the peak
POINT DetectTranslation(const Image& a, const Image& b, double* confidence = nullptr);

} // namespace snap::cv
