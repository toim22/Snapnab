// Visão computacional própria (equivalente de TSCV.*, ds_arch §11 / ds_pseudocode §9)
#pragma once
#include "Image.h"

namespace snap::cv {

using Mask = std::vector<uint8_t>;   // w*h, 0 ou 255

// §9.1 WandSelector::Select — crescimento de região ordenado por custo (Dijkstra com limite)
//   tolerance ∈ (0, 100]; global = GlobalWandSelector (todos os pixels de cor semelhante)
Mask WandSelect(const Image& img, POINT seed, float tolerance, bool global);
// §10.2 DoWand: clamp [0.01, 100] e atalho 100 → camada inteira
Mask DoWand(const Image& img, POINT seed, float tolerance, bool global);

// máscaras auxiliares
Mask RectMask(int w, int h, RECT r);
Mask EllipseMask(int w, int h, RECT r);
Mask PolygonMask(int w, int h, const std::vector<POINT>& pts);
RECT MaskBounds(const Mask& m, int w, int h);
bool MaskEmpty(const Mask& m);
Mask Dilate(const Mask& m, int w, int h, int k);

// §9.4 Dingo7Inpainter — pinta a seleção com a cor dominante da vizinhança dilatada
bool Dingo7Inpaint(Image& img, const Mask& mask, int kernel, uint32_t* dominantOut = nullptr, double* shareOut = nullptr);
// Difusão (papel do NavierStokesInpainter / cv::inpaint): preenche por iteração de Laplace a partir da borda
void DiffusionInpaint(Image& img, const Mask& mask, int iterations = 0);
// Smart fill: Dingo7 quando a vizinhança é praticamente uniforme; senão difusão
void SmartFill(Image& img, const Mask& mask);

// §9.6 tolerância automática do Smart Move
float SmartMoveAutoTolerance(const Image& img);
float SmartMoveToleranceScaling(float t, float inMin, float inMax, float outMin, float outMax);

// §9.5 PhaseCorrelationTranslationDetector — deslocamento (dx,dy) de b em relação a a
//   devolve o pico da correlação de fase; confidence = altura relativa do pico
POINT DetectTranslation(const Image& a, const Image& b, double* confidence = nullptr);

} // namespace snap::cv
