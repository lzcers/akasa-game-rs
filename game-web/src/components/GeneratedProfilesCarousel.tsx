import React, { useEffect, useRef, useState } from "react";
import type { GeneratedProfilePanel } from "./generatedProfilePanels";

interface GeneratedProfilesCarouselProps {
  panels: GeneratedProfilePanel[];
  resetKey: string;
  className?: string;
  panelClassName?: string;
}

const GeneratedProfilesCarousel: React.FC<GeneratedProfilesCarouselProps> = ({
  panels,
  resetKey,
  className = "",
  panelClassName = "",
}) => {
  const carouselRef = useRef<HTMLDivElement>(null);
  const carouselDragRef = useRef({
    isDragging: false,
    startX: 0,
    scrollLeft: 0,
  });
  const [isCarouselDragging, setIsCarouselDragging] = useState(false);
  const [activeCursor, setActiveCursor] = useState({ key: "", index: 0 });
  const activeIndex =
    activeCursor.key === resetKey
      ? Math.min(activeCursor.index, Math.max(panels.length - 1, 0))
      : 0;

  useEffect(() => {
    const carousel = carouselRef.current;
    if (carousel) {
      carousel.scrollTo({ left: 0 });
    }
  }, [resetKey]);

  const handleProfileScroll = () => {
    const carousel = carouselRef.current;
    if (!carousel) {
      return;
    }

    const nextIndex = Math.round(
      carousel.scrollLeft / Math.max(carousel.clientWidth, 1),
    );
    setActiveCursor({
      key: resetKey,
      index: Math.min(Math.max(nextIndex, 0), Math.max(panels.length - 1, 0)),
    });
  };

  const scrollToProfile = (index: number) => {
    const carousel = carouselRef.current;
    if (!carousel) {
      return;
    }

    carousel.scrollTo({
      left: carousel.clientWidth * index,
      behavior: "smooth",
    });
    setActiveCursor({
      key: resetKey,
      index,
    });
  };

  const snapToNearestProfile = () => {
    const carousel = carouselRef.current;
    if (!carousel) {
      return;
    }

    const nextIndex = Math.round(
      carousel.scrollLeft / Math.max(carousel.clientWidth, 1),
    );
    scrollToProfile(
      Math.min(Math.max(nextIndex, 0), Math.max(panels.length - 1, 0)),
    );
  };

  const handleCarouselPointerDown = (
    event: React.PointerEvent<HTMLDivElement>,
  ) => {
    if (event.pointerType === "mouse" && event.button !== 0) {
      return;
    }

    const carousel = event.currentTarget;
    carouselDragRef.current = {
      isDragging: true,
      startX: event.clientX,
      scrollLeft: carousel.scrollLeft,
    };
    carousel.setPointerCapture(event.pointerId);
    setIsCarouselDragging(true);
  };

  const handleCarouselPointerMove = (
    event: React.PointerEvent<HTMLDivElement>,
  ) => {
    const drag = carouselDragRef.current;
    if (!drag.isDragging) {
      return;
    }

    const deltaX = event.clientX - drag.startX;
    if (Math.abs(deltaX) > 3) {
      event.preventDefault();
    }
    event.currentTarget.scrollLeft = drag.scrollLeft - deltaX;
  };

  const finishCarouselDrag = (event: React.PointerEvent<HTMLDivElement>) => {
    if (!carouselDragRef.current.isDragging) {
      return;
    }

    carouselDragRef.current.isDragging = false;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    setIsCarouselDragging(false);
    snapToNearestProfile();
  };

  if (panels.length === 0) {
    return null;
  }

  return (
    <div className={`flex min-h-0 flex-1 flex-col gap-2 ${className}`}>
      <div
        ref={carouselRef}
        onScroll={handleProfileScroll}
        onPointerDown={handleCarouselPointerDown}
        onPointerMove={handleCarouselPointerMove}
        onPointerUp={finishCarouselDrag}
        onPointerCancel={finishCarouselDrag}
        onPointerLeave={finishCarouselDrag}
        className={`flex min-h-0 flex-1 snap-x snap-mandatory gap-2 overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden ${
          isCarouselDragging
            ? "cursor-grabbing scroll-auto select-none"
            : "cursor-grab scroll-smooth"
        }`}
      >
        {panels.map((panel) => (
          <article
            key={panel.key}
            id={panel.key}
            className={`flex h-full min-h-0 w-full min-w-full snap-center flex-col rounded-xl border p-3 md:p-4 ${panel.className} ${panelClassName}`}
          >
            <h2 className="shrink-0 text-base font-semibold leading-6 text-[#f8f1e3] md:text-lg">
              {panel.title}
            </h2>
            <p className="akashic-scroll mt-2 min-h-0 flex-1 overflow-y-auto whitespace-pre-wrap pr-1 text-sm leading-6 sm:text-[0.95rem] sm:leading-7 md:text-base">
              {panel.text}
            </p>
          </article>
        ))}
      </div>

      <div className="flex shrink-0 justify-center gap-1">
        {panels.map((panel, index) => (
          <button
            key={panel.key}
            type="button"
            aria-label={`切换到${panel.title}`}
            aria-current={activeIndex === index ? "true" : undefined}
            onClick={() => scrollToProfile(index)}
            className="flex h-8 min-w-8 items-center justify-center rounded-full transition-colors hover:bg-white/8"
          >
            <span
              className={`h-1.5 rounded-full transition-all ${
                activeIndex === index ? "w-7 bg-[#d8c58a]" : "w-3 bg-white/25"
              }`}
            />
          </button>
        ))}
      </div>
    </div>
  );
};

export default GeneratedProfilesCarousel;
