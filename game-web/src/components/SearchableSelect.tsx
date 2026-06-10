import React from 'react';
import { ChevronDown } from 'lucide-react';

interface SearchableSelectProps {
  value: string;
  options: readonly string[];
  placeholder: string;
  createText: string;
  onChange: (value: string) => void;
}

const SearchableSelect: React.FC<SearchableSelectProps> = ({
  value,
  options,
  placeholder,
  createText,
  onChange,
}) => {
  const containerRef = React.useRef<HTMLDivElement>(null);
  const [isOpen, setIsOpen] = React.useState(false);

  React.useEffect(() => {
    const handlePointerDown = (event: MouseEvent) => {
      if (!containerRef.current?.contains(event.target as Node)) {
        setIsOpen(false);
      }
    };

    document.addEventListener('mousedown', handlePointerDown);
    return () => document.removeEventListener('mousedown', handlePointerDown);
  }, []);

  const trimmedValue = value.trim();
  const hasExactMatch = options.some((option) => option === trimmedValue);
  const canKeepCustomValue = trimmedValue.length > 0 && !hasExactMatch;

  return (
    <div ref={containerRef} className="relative">
      <div className="relative">
        <input
          type="text"
          value={value}
          onFocus={() => setIsOpen(true)}
          onChange={(e) => {
            onChange(e.target.value);
            setIsOpen(true);
          }}
          onKeyDown={(e) => {
            if (e.key === 'Escape') {
              setIsOpen(false);
            }
            if (e.key === 'Enter') {
              setIsOpen(false);
            }
          }}
          className="akashic-field pr-11"
          placeholder={placeholder}
        />
        <button
          type="button"
          onClick={() => setIsOpen((open) => !open)}
          className="absolute inset-y-0 right-0 flex w-11 items-center justify-center text-[#c8b392] transition-colors hover:text-[#efe4cd]"
          aria-label="展开备选"
        >
          <ChevronDown className={`h-4 w-4 transition-transform ${isOpen ? 'rotate-180' : ''}`} />
        </button>
      </div>

      {isOpen ? (
        <div className="absolute inset-x-0 top-[calc(100%+0.45rem)] z-30 overflow-hidden rounded-2xl border border-[#6f6655]/55 bg-[#0d1627]/96 shadow-[0_16px_36px_rgba(2,8,18,0.48)] backdrop-blur-xl">
          {canKeepCustomValue ? (
            <button
              type="button"
              onClick={() => {
                onChange(value);
                setIsOpen(false);
              }}
              className="flex w-full items-center justify-between gap-3 border-b border-white/8 px-3.5 py-3 text-left transition-colors hover:bg-white/5"
            >
              <span className="text-sm text-[#efe4cd]">{createText}</span>
              <span className="truncate text-xs text-[#9ca7be]">{trimmedValue}</span>
            </button>
          ) : null}

          <div className="max-h-56 overflow-y-auto py-1.5">
            {options.map((option) => (
              <button
                key={option}
                type="button"
                onClick={() => {
                  onChange(option);
                  setIsOpen(false);
                }}
                className={`block w-full px-3.5 py-2.5 text-left text-sm transition-colors hover:bg-white/5 ${option === trimmedValue ? 'bg-white/6 text-[#f6eddc]' : 'text-[#d7c7ab]'}`}
              >
                {option}
              </button>
            ))}
          </div>
        </div>
      ) : null}
    </div>
  );
};

export default SearchableSelect;
